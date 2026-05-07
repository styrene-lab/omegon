+++
id = "52e71bf9-e0a5-4306-8a42-6d0ed13149a8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Vox Communication Extension

The vox extension connects you to external communication channels (Discord, Slack, Signal, email, etc.). Messages from these channels arrive as prompts with embedded routing context.

## Handling inbound messages

When you receive a message containing a `<vox_reply_context>` block, it is an inbound message from a communication channel routed through vox. The block contains a `reply_address` (routing information for the response) and a `session_key` (sender identity).

**Always respond using the `vox_reply` tool.** Pass the `reply_address` object exactly as provided and your response as the `text` parameter. Do not modify the reply_address — it contains the channel, envelope, thread, and protocol hints needed to route your response back to the correct conversation.

Example:
```
vox_reply(reply_address=<the reply_address object from the context block>, text="Your response here")
```

## Guidelines

- Respond to every inbound message. Users on the other end of these channels are waiting for a reply.
- Keep responses concise and appropriate for the channel (Discord messages have a 2000 character limit).
- The session_key identifies the sender and conversation. Different session_keys are different users or threads.
- You may receive messages from multiple users interleaved. Use the session_key to track who you are responding to.
- If a message has no `<vox_reply_context>`, it is a normal prompt — not from vox. Do not use vox_reply for those.
