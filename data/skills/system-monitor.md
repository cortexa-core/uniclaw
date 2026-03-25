---
name: system-monitor
description: System monitoring and diagnostics patterns
tags: [system, cpu, memory, disk, temperature, status, health, uptime]
priority: 50
requires:
  tools: [system_info, shell_exec]
---

## System Monitoring

When asked about system health or status:
1. Use `system_info` for OS, CPU, memory, temperature
2. Use `shell_exec` with `df -h` for disk usage
3. Use `shell_exec` with `uptime` for load average
4. Present as a clean summary, highlight any concerns
