# Descriptor-bound recovery

## Evidence and authority

The observed legacy record has the same canonical home path and inode but a
changed device number. The historical cause remains unknown and is not a
prerequisite for explicit operator recovery. Path-derived contribution, session,
and audit keys do not include the device; they must remain unchanged. Recovery
is narrowly restricted to the same path and inode, with an explicit command
providing the legacy continuity decision. Different paths, inodes, corrupt
records, or conflicting persisted continuity evidence are refused.

## Command and guards

`omegon-maintain home inspect` is read-only. `home recover --dry-run --deadline
30s` inspects eligibility without persistent writes. `home recover --deadline
30s --request-id <uuid>` explicitly applies or resumes the transaction. Existing
CLI root grants and deadline contracts apply. Neither inspection nor recovery
loads providers, plugins, configuration, or runtime contributions.

Inspection holds bootstrap.lock shared without taking domain locks. Apply and
dry-run hold bootstrap.lock exclusively, then acquire every existing protocol lock
nonblocking, including audit.lock. Admission already takes bootstrap before
creating/acquiring a scope or session lock. Holding bootstrap and all existing
locks therefore excludes both new and retained admissions; lock contention
refuses immediately, avoiding lock-order inversion with existing mutations.
Refuse outstanding fences and nonterminal transactions. A cached state object
must also check the recovery journal while acquiring admission under bootstrap.
All enumeration and reads remain descriptor-relative, bounded, no-follow, and
user-owned. Recheck the home and maintenance directory descriptors before writes,
and validate installation contents against the exact original or target record.

## Persistence and crash settlement

Keep an immutable recovery intent containing the complete original installation
record, target identity, optional stable continuity binding, and request ID.
Maintain a separate phase journal. Persist the pending journal before the
immutable intent, and both before replacing installation state. A crash before
the intent write can only recreate it when the current original-state digest
matches the prepared journal. Pending recovery blocks ordinary bootstrap and
cached admission, including cached maintenance mutation dispatch. Replace state atomically and fsync through the existing record
writer; retain UUID, record ID, next audit sequence, deny/session directories and
all keys. Append one existing-protocol audit event with a receipt, then mark the
journal settled. Replaying the request resumes from observed state and audit
receipt, never invents a second installation or duplicates an audit event.
Conflicting request IDs, original/target state edits, or ambiguous records refuse.

## Stable continuity

On macOS, obtain volume UUID with fgetattrlist on the opened home descriptor.
Persist UUID together with the canonical path and inode, bound to the exact
installation record identity. Future startup may accept changed device numbers
only when that persisted evidence still matches the opened descriptor. Existing
legacy records are not silently upgraded through a mismatch. Unsupported
platforms keep strict device checks and explicit recovery. Directory rename,
replacement, volume mismatch, and tampered bindings remain failures. No global
weakening of PathIdentityV1 equality or descriptor race checks is permitted.

## Companion descriptor budget

The observed home has 439 domain lock files; macOS launches the companion with a
soft descriptor limit of 256 and an unlimited hard limit. Recovery still needs
all locks simultaneously. After bounded inventory and before acquiring domain
locks, reserve one descriptor per lock plus 64 for roots, audit, journal, and
existing process descriptors. A scoped guard raises only RLIMIT_NOFILE's soft
limit when needed, preserves the hard limit, and restores the original soft
limit when recovery access is released, including failure paths. If the hard
limit or kernel policy prevents this budget, refuse before recovery records are
written. Child-process tests alter limits only inside their isolated process;
the operator shell and normal harness limits are never changed.
