use crate::domain::{Attachment, Focus, Message, MessageId, Provider, ReplyMode, ReplyTarget};

pub fn messages() -> Vec<Message> {
    [
        (1, Provider::ICloud, Focus::Personal, "Mira", "Sunday by the water", "Shall we bring a picnic? I can bring bread and fruit.\n\nAll names and conversations in this demo are fictional."),
        (2, Provider::Discord, Focus::Work, "Rowan", "Design room · spacing pass", "The latest spacing pass is ready for review. A little breathing room goes a long way.\n\nThis fixture represents an authorised bot conversation, not a personal account mirror."),
        (3, Provider::Teams, Focus::Work, "Jules", "Release checklist", "The preview build is ready. Can you check the keyboard flow before our next review?\n\nTry browsing another conversation while typing a reply here."),
        (4, Provider::Office365, Focus::Work, "Sasha", "A calmer communications drawer", include_str!("../fixtures/long-email.txt")),
        (5, Provider::ICloud, Focus::Personal, "Kai", "A small weekend expedition 🌿", "A walk, a coffee, and absolutely no agenda. Are you in?\n\nUnicode sample: café · 東京 · 👩🏽‍💻 · e\u{301}"),
    ].into_iter().map(|(id, provider, focus, sender, subject, body)| {
        let account = if focus == Focus::Work { "demo-work" } else { "demo-personal" };
        Message {
            id: MessageId(id), timestamp: 1_000 + id, time: format!("10:{:02}", 35 + id),
            sender: sender.into(), subject: subject.into(), body: body.into(), focus,
            unread: true, incoming: true,
            target: Some(ReplyTarget {
                provider, account: account.into(), conversation: format!("fixture-thread-{id}"),
                incoming: MessageId(id), recipients: vec![format!("{}@example.invalid", sender.to_lowercase())],
                reply_mode: ReplyMode::Sender,
            }),
        }
    }).collect()
}

pub fn arrival(number: u64) -> Message {
    let mut message = messages().remove(2);
    message.id = MessageId(100 + number);
    message.timestamp = 2_000 + number;
    message.time = format!("11:{:02}", number % 60);
    message.subject = format!("Simulated arrival #{number}");
    message.body = "A fictional incoming message. An active or locked composer must keep its existing recipient.".into();
    if let Some(target) = &mut message.target {
        target.incoming = message.id;
        target.conversation = format!("fixture-arrival-{number}");
    }
    message
}

pub fn find(id: MessageId) -> Option<Message> {
    messages()
        .into_iter()
        .find(|m| m.id == id)
        .or_else(|| (id.0 > 100 && id.0 <= 10_100).then(|| arrival(id.0 - 100)))
}

pub fn attachment() -> Attachment {
    Attachment {
        name: "spacing-notes.txt".into(),
        bytes: include_bytes!("../fixtures/spacing-notes.txt").len() as u64,
        mime: "text/plain".into(),
    }
}
