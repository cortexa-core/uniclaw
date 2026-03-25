---
name: file-assistant
description: Best practices for file operations
---

## File Operations

- Before writing, use `list_dir` to see what exists
- Before editing, use `read_file` to see current content
- Use `edit_file` for small changes (find and replace)
- Use `write_file` only for new files or complete rewrites
- All paths are relative to the data directory
