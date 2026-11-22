---
name: issue-hygiene
description: Use when working from, updating, cleaning up, or closing a GitHub issue, sub-issue, or parent tracking issue, including blockers, relationships, labels, or comments.
---

# GitHub issue hygiene

Keep issues concise to prevent context rot and downstream confusion. Use the GitHub CLI.

Read the complete issue body, comments, labels, linked issues, and linked pull requests, and research any open branches or worktrees relevant to the issue. Determine whether the issue is active, blocked, duplicated, already completed, obsolete, or unclear before acting.

## Issue body

The body is the concise current source of truth. Improve the title if it does not clearly describe the problem or desired outcome. Use only the sections that are useful:

- `## Description`: no more than three sentences.
- `## Decisions and Constraints`: a list, only to reduce length.
- `## Status`: a Markdown table, only when meaningful work is complete and the issue is open. Remove it when the issue closes.
- `## Blockers`: a Markdown table, only while blockers exist. Remove it when the issue closes.

Use Markdown tables for Status and Blockers to separate data from prose within a row.

## Comments

Comments record only durable context that will be encoded into the project: technical facts and design decisions. Comments do not record status.

## Blockers and relationships

Mention a blocker in the body and represent it with GitHub's native relationship: `blocked by` on the blocked issue and the corresponding `blocks` on the other, only while the block actually holds. The relationship is the machine-readable record. Record related issues with the native relationship only; do not record them in the body.

## Parent tracking issues

A parent body is an index, not a narrative: a description of what the tracker covers and a list of its open sub-issues as bare links with titles. Only sub-issues of that parent may appear, and only while open; delete a closed sub-issue's line outright rather than keeping checked boxes, strikethroughs, or shipped sections. Discussion of an individual sub-issue's design, status, evidence, or history belongs on the sub-issue; this applies to the parent's comments as well as its body. Express sequencing with the blocking relationship, not prose annotations. Close the parent only when every sub-issue is closed and no parent-level work remains.

## Cleaning up an existing issue

Consolidate durable information from comments into the body: decisions, technical findings, constraints, unresolved questions, and validation evidence. Remove obsolete chronology, repeated status updates, abandoned plans, and conversational filler. Preserve the issue's original intent; do not invent requirements, conclusions, or completion claims. Preserve existing labels unless one is clearly incorrect, and do not introduce a new label taxonomy. Do not post a routine cleanup comment.

Never delete a human-authored comment. Delete an agent-authored comment only when its entire value is transient progress reporting, everything useful is preserved in the updated body, and it contains no decision, rationale, instruction, technical finding, disagreement, or validation evidence. Record a validation finding itself, not incidental details of how it was found.

Immediately before editing, re-read the issue to avoid overwriting newer changes. If you are unsure about anything, stop and ask for human review.
