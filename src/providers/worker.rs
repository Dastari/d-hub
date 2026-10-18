use super::*;
use crate::{composer::Submission, read_state::ReadChange};
use std::{
    collections::BTreeMap,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub enum Task {
    Sync(Value),
    Read(ReadChange),
    Send(Box<(Item, Submission)>),
}
pub enum Outcome {
    Sync(Result<Batch>),
    Read(ReadChange, Result<()>),
    Send(u64, SendResult),
}
pub struct Completion {
    pub account: AccountConfig,
    pub outcome: Outcome,
}
struct Worker {
    account: AccountConfig,
    tx: SyncSender<Task>,
    rx: Receiver<Completion>,
    busy: bool,
    inflight: Option<Task>,
    dead: bool,
    next: Instant,
    read_next: Instant,
    last_read: bool,
}
pub type Factory = fn() -> Result<Box<dyn Transport>>;
pub struct Workers {
    workers: BTreeMap<String, Worker>,
    factory: Factory,
}
fn network() -> Result<Box<dyn Transport>> {
    Ok(Box::new(Network::new()?))
}
impl Default for Workers {
    fn default() -> Self {
        Self::new(network)
    }
}
impl Workers {
    pub fn new(factory: Factory) -> Self {
        Self {
            workers: BTreeMap::new(),
            factory,
        }
    }
    pub fn refresh(&mut self) {
        for worker in self.workers.values_mut() {
            worker.next = Instant::now();
        }
    }
    pub fn collect(&mut self) -> Vec<Completion> {
        let mut completed = Vec::new();
        for worker in self.workers.values_mut() {
            let completion = match worker.rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) if worker.busy => {
                    worker.dead = true;
                    let outcome = match worker.inflight.take() {
                        Some(Task::Send(op)) => {
                            Outcome::Send(op.1.operation, SendResult::Uncertain)
                        }
                        Some(Task::Read(change)) => Outcome::Read(
                            change,
                            Err(anyhow::anyhow!(
                                "Provider worker stopped; read change retained"
                            )),
                        ),
                        _ => Outcome::Sync(Err(anyhow::anyhow!("Provider worker stopped"))),
                    };
                    Some(Completion {
                        account: worker.account.clone(),
                        outcome,
                    })
                }
                _ => None,
            };
            if let Some(result) = completion {
                worker.inflight = None;
                worker.busy = false;
                let delay = match &result.outcome {
                    Outcome::Sync(Ok(_))
                        if worker.account.provider == Provider::Discord
                            && worker.account.discord_mode == "personal" =>
                    {
                        1
                    }
                    Outcome::Sync(Err(e)) | Outcome::Read(_, Err(e)) => e
                        .downcast_ref::<graph::Throttled>()
                        .map(|e| e.0)
                        .unwrap_or(worker.account.poll_seconds),
                    _ => worker.account.poll_seconds,
                };
                match &result.outcome {
                    Outcome::Sync(_) => {
                        worker.next = Instant::now() + Duration::from_secs(delay.min(86_400))
                    }
                    Outcome::Read(_, r) => {
                        worker.read_next = Instant::now()
                            + if r.is_ok() {
                                Duration::ZERO
                            } else {
                                Duration::from_secs(delay.min(86_400))
                            }
                    }
                    _ => {}
                }
                if let Outcome::Sync(Err(e)) | Outcome::Read(_, Err(e)) = &result.outcome
                    && let Some(throttle) = e.downcast_ref::<graph::Throttled>()
                {
                    let until = Instant::now() + Duration::from_secs(throttle.0.min(86_400));
                    worker.next = until;
                    worker.read_next = until;
                }
                completed.push(result);
            }
        }
        completed
    }
    pub fn schedule(
        &mut self,
        accounts: &[AccountConfig],
        cursors: &std::collections::BTreeMap<String, Value>,
        reads: &std::collections::BTreeMap<MessageId, ReadChange>,
        send: Option<(Item, Submission)>,
    ) -> bool {
        // Never replace a busy account worker: a non-idempotent request may be in progress.
        self.workers.retain(|id, w| {
            w.busy
                || !w.dead
                    && accounts
                        .iter()
                        .any(|a| a.id == *id && a.enabled && a.same_connection(&w.account))
        });
        let mut started_send = false;
        for account in accounts.iter().filter(|a| a.enabled) {
            if !self.workers.contains_key(&account.id) {
                let (tx, rx) = mpsc::sync_channel(1);
                let (result, results) = mpsc::sync_channel(1);
                let account = account.clone();
                let copy = account.clone();
                let factory = self.factory;
                std::thread::spawn(move || {
                    let mut transport = factory();
                    while let Ok(task) = rx.recv() {
                        let outcome = match task {
                            Task::Sync(cursor) => Outcome::Sync(match &mut transport {
                                Ok(t) => t.sync(&copy, cursor),
                                Err(_) => {
                                    Err(anyhow::anyhow!("Provider worker could not initialize"))
                                }
                            }),
                            Task::Read(change) => {
                                let response = match &mut transport {
                                    Ok(t) => t.unread(&copy, &change.remote, change.unread),
                                    Err(_) => Err(anyhow::anyhow!("Provider worker unavailable")),
                                };
                                Outcome::Read(change, response)
                            }
                            Task::Send(operation) => {
                                let (item, op) = *operation;
                                Outcome::Send(
                                    op.operation,
                                    match &mut transport {
                                        Ok(t) => t.send(&copy, &item, &op),
                                        Err(_) => {
                                            SendResult::Failed("Provider worker unavailable".into())
                                        }
                                    },
                                )
                            }
                        };
                        if result
                            .send(Completion {
                                account: copy.clone(),
                                outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                self.workers.insert(
                    account.id.clone(),
                    Worker {
                        account,
                        tx,
                        rx: results,
                        busy: false,
                        inflight: None,
                        dead: false,
                        next: Instant::now(),
                        read_next: Instant::now(),
                        last_read: false,
                    },
                );
            }
            let worker = self.workers.get_mut(&account.id).expect("worker exists");
            if worker.busy || worker.account != *account {
                continue;
            }
            let task = if let Some((item, op)) = &send
                && op.draft.target.account == account.id
            {
                started_send = true;
                Some(Task::Send(Box::new((item.clone(), op.clone()))))
            } else {
                let read = reads.values().find(|r| {
                    remote_account(&r.remote) == account.id
                        && r.account_scope
                            .as_deref()
                            .is_none_or(|s| s == scope(account))
                });
                if let Some(read) = read
                    && Instant::now() >= worker.read_next
                    && (Instant::now() < worker.next || !worker.last_read)
                {
                    worker.last_read = true;
                    Some(Task::Read(read.clone()))
                } else if Instant::now() >= worker.next {
                    worker.last_read = false;
                    Some(Task::Sync(
                        cursors.get(&scope(account)).cloned().unwrap_or(Value::Null),
                    ))
                } else {
                    None
                }
            };
            if let Some(task) = task {
                if worker.tx.try_send(task.clone()).is_ok() {
                    worker.busy = true;
                    worker.inflight = Some(task);
                } else {
                    started_send = false;
                }
            }
        }
        started_send
    }
}
pub fn remote_account(remote: &RemoteMessage) -> &str {
    match remote {
        RemoteMessage::Imap { account, .. }
        | RemoteMessage::Graph { account, .. }
        | RemoteMessage::DiscordDm { account, .. }
        | RemoteMessage::GraphChat { account, .. } => account,
    }
}
