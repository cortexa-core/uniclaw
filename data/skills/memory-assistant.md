---
name: memory-assistant
description: Guide for when and how to use long-term memory
always: true
priority: 90
---

## Memory Management

- When the user shares personal info, preferences, or facts they want remembered, use `memory_store` with a descriptive key
- When the user asks "do you remember" or "what do you know about me", use `memory_read` first
- Good keys: user_name, preference_units, favorite_color, project_name
- Don't store trivial conversation (greetings, one-off questions)
