Tell the decisions who you are, and word the for-you questions yourself.

- **About me** (Settings › AI decisions, or `:about`): a few lines about you, such as your name, work and the groups you're in. They're sent with every decision, so "meant for you or a group you're in" has something to go on. It's empty by default, and then nothing extra is sent.
- The two for-you questions are editable, along with what a yes and a no mean: "Written by a person?" (`:spam person`) and "Meant for you?" (`:spam for-you`). Both are on Settings › Filtering and in `config` (`spam.person_*`, `spam.for_you_*`). They start with the previous wording, and a blank description is left out.
- Changing a question or About me asks again, so answers to the old wording are never reused.

Decision quality on real mail is untested; adjust wording and `spam.for_you_threshold` on your own messages. Real-account acceptance for Teams, Office 365 and iCloud is still pending the owner's sign-in. No license has been selected.
