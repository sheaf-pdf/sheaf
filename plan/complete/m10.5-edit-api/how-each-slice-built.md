# M10.5: How each slice is built

Part of [M10.5: the edit API](README.md).


Each slice is built by a workflow of agents, run the same way every
time, so every slice has had the same scrutiny:

1. **Plan.** A planner reads the slice, every design section,
   proposal, bug and ledger row it cites, and the earlier slices'
   notes, then the code. It lists where the plan and the code disagree,
   and breaks the slice into 3–8 ordered steps that each leave the
   workspace compiling, with every Check and Deletes item mapped to a
   step.
2. **Plan review** (from slice 9). A second planner attacks the first
   plan against the slice and the code (a missed Check or Delete, a
   step that can't compile alone, a wrong order, a misread of the
   design) and returns the plan revised. Greg gets a condensed copy;
   the steps start right away.
3. **Implement.** One implementer per step, in order. Each reads the
   slice and the earlier steps' reports, writes the code, its tests and
   its deletions, and runs the builds and tests. The last step writes
   the slice's notes.
4. **Review.** Four adversarial reviewers read the uncommitted diff in
   parallel: conformance to the plan (every scope bullet, Delete and
   Check), correctness (bugs, hostile input, undo and replay, PDF
   validity, regressions), speedups (every fast path attacked: does it
   write what the slow path would, and does the corpus still hold), and
   tests and quality (would each test fail if its behavior broke;
   naming, comments, dead code).
5. **Fix.** A fixer checks each finding against the code, fixes the real
   ones (with tests where a finding shows a gap) and rejects the rest
   with a reason. Deviations it adds go into the slice notes.
6. **Verify.** A verifier runs `cargo fmt --check`, `cargo clippy
   --workspace --all-targets -D warnings`, `cargo test --workspace`,
   the corpus suite and the ignored corpus tiers (from slice 10:
   `cargo test -p sheaf-ops --test append_corpus -- --ignored`), removes
   stray files, checks the notes against the
   diff, and drafts the commit message.
7. **Commit.** The orchestrator runs the same checks again (the
   ignored corpus tiers included), plus the ignored interop tier (qpdf and pdf.js) when a slice writes files,
   then commits the slice as one commit and pushes it.

Every agent works to two rules. **Fast and correct:** a speedup goes
in only when it's shown to write the same bytes (the corpus identity
and determinism runs, and a test of its fast path's condition); no
check, read-back or test is weakened, and no corpus file left out, to
meet a gate. A corpus tier that takes minutes goes behind `#[ignore]`,
still over every file, and Verify and Commit run it (Greg,
2026-10-19). A gate that correct code can't meet is reported for Greg
to decide. **Bounded tests:** every `cargo test` runs its binaries in a
memory-capped scope (`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=
'systemd-run --user --scope -p MemoryMax=48G -p MemorySwapMax=0 --'`),
so a runaway test dies alone (slice 9's u32::MAX-numbered xref grew
to 158 GB and took the session down with it). The corpus identity
counts are asserted (`materialize_corpus::IDENTITY`); a slice that
moves them says why in its notes.

What each slice's review found, and what was fixed or deviated from, is
in its slice notes. Only Linux runs locally (Windows gets `cargo check
--target x86_64-pc-windows-msvc`); CI on each PR runs Linux, Windows and
macOS.

**Six PRs.** M10.5 reaches `main` in six PRs, one per phase, each
ending with the app fully working: foundations (slices 1–7), model and
saving (8–10), the edit vocabulary (11–12), rendering (13–14), the
feature moves (15–18) and the consumers (19–20). Each phase has its own
branch (`m10.5`, then `m10.5-p2` and so on). PRs are squash-merged;
after each merge the next branch is rebased onto `main` with `git rebase
--onto origin/main <previous branch's tip> <next branch>`, its tests run
again, and it is pushed.
