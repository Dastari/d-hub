use crate::domain::{Attachment, ReplyTarget, safe_text};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub target: ReplyTarget,
    pub body: String,
    pub attachments: Vec<Attachment>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub operation: u64,
    pub draft: Draft,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendState {
    #[default]
    Ready,
    Submitting(Submission),
    Failed(String),
    Uncertain(Submission),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composer {
    pub target: Option<ReplyTarget>,
    pub draft: Option<Draft>,
    pub send: SendState,
    pub sent: Vec<Submission>,
    pub next_operation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    Select(Option<ReplyTarget>),
    Body(String),
    Attach(Attachment),
    RemoveAttachment(usize),
    Discard(Option<ReplyTarget>),
    Submit,
    Accepted(u64),
    Failed(u64, String),
    Uncertain(u64),
    NotSent(u64),
}

impl Composer {
    pub fn editable(&self) -> bool {
        matches!(self.send, SendState::Ready | SendState::Failed(_))
    }

    pub fn body(&self) -> &str {
        self.draft.as_ref().map_or("", |d| d.body.as_str())
    }

    pub fn reply_target(&self) -> Option<&ReplyTarget> {
        self.draft
            .as_ref()
            .map(|d| &d.target)
            .or(self.target.as_ref())
    }

    fn lock(&mut self) -> Option<&mut Draft> {
        if self.draft.is_none() {
            self.draft = Some(Draft {
                target: self.target.clone()?,
                body: String::new(),
                attachments: vec![],
                revision: 0,
            });
        }
        self.draft.as_mut()
    }

    /// Pure reducer. Only Submit returns a frozen operation for a transport to execute.
    pub fn apply(&mut self, event: Event) -> Option<Submission> {
        match event {
            Event::Select(target) if self.draft.is_none() => self.target = target,
            Event::Body(body) if self.editable() => {
                let body = safe_text(&body);
                if (self.draft.is_some() || !body.is_empty())
                    && let Some(draft) = self.lock()
                {
                    draft.body = body;
                    draft.revision += 1;
                    self.send = SendState::Ready;
                }
            }
            Event::Attach(mut attachment) if self.editable() => {
                attachment.name = safe_text(&attachment.name);
                if let Some(draft) = self.lock() {
                    draft.attachments.push(attachment);
                    draft.revision += 1;
                    self.send = SendState::Ready;
                }
            }
            Event::RemoveAttachment(index) if self.editable() => {
                if let Some(draft) = &mut self.draft
                    && index < draft.attachments.len()
                {
                    draft.attachments.remove(index);
                    draft.revision += 1;
                    self.send = SendState::Ready;
                }
            }
            Event::Discard(target) if self.editable() => {
                self.draft = None;
                self.target = target;
                self.send = SendState::Ready;
            }
            Event::Submit if self.editable() => {
                let draft = self.draft.as_ref()?;
                if draft.body.trim().is_empty() && draft.attachments.is_empty() {
                    return None;
                }
                self.next_operation += 1;
                let submission = Submission {
                    operation: self.next_operation,
                    draft: draft.clone(),
                };
                self.send = SendState::Submitting(submission.clone());
                return Some(submission);
            }
            Event::Accepted(id) => {
                if let SendState::Submitting(operation) | SendState::Uncertain(operation) =
                    &self.send
                    && operation.operation == id
                {
                    let operation = operation.clone();
                    self.target = Some(operation.draft.target.clone());
                    self.sent.push(operation);
                    self.draft = None;
                    self.send = SendState::Ready;
                }
            }
            Event::Failed(id, error) => {
                if let SendState::Submitting(operation) = &self.send
                    && operation.operation == id
                {
                    self.send = SendState::Failed(safe_text(&error));
                }
            }
            Event::NotSent(id) => {
                if matches!(&self.send,SendState::Uncertain(op) if op.operation==id) {
                    self.send =
                        SendState::Failed("Owner confirmed not sent; draft unlocked".into());
                }
            }
            Event::Uncertain(id) => {
                if let SendState::Submitting(operation) = &self.send
                    && operation.operation == id
                {
                    self.send = SendState::Uncertain(operation.clone());
                }
            }
            _ => {}
        }
        None
    }

    pub fn recover(&mut self) {
        if let SendState::Submitting(operation) = &self.send {
            self.send = SendState::Uncertain(operation.clone());
        }
    }
}
