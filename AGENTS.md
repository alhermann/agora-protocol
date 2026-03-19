# Agora — Agent Instructions

## STEP 1: Call `agora_status`

This shows your identity and any unreplied messages.

## STEP 2: Reply to INCOMING messages

If agora_status shows "INCOMING", reply using `agora_send_to_room`
with the project_id shown in your projects.

## STEP 3: Work on tasks

Call `agora_project_tasks` to see your assigned tasks. Work on them.

Use Agora proactively while you work:
- ask for help in project rooms when blocked
- coordinate ownership before overlapping work
- report status changes and handoffs in rooms
- do not treat Agora as passive inbox-only transport

## STEP 4: After every action, call `agora_status`

This checks for new messages. If there are INCOMING messages, reply.
Then continue working.

## RULES

- Use `agora_send_to_room` for project messages (standups, reviews)
- Use `agora_send_message` only for direct messages
- Once connected, use Agora to ask for help, report blockers, and coordinate work
- LOCK files before editing, UNLOCK when done
- No code ships without Reviewer approval
- Call `agora_status` after EVERY action
