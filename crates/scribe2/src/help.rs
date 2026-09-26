//! 人が端末で読む案内 `help [<command>]` と、`--help` / `-h` の pointer の 1 行（設計 docs/design/cli-help.md §2）。
//!
//! 正本は表 [`TABLE`] 1 つ（1 command 1 行・欄は name / purpose / what / form / subcommands / flags / examples / see）で、
//! 描画の関数は表を読むだけである。案内は英語で書き、規則を述べない（規則の在処は `see` の設計 doc）。各 module の使い方の
//! 1 行は今の場所のまま持ち、`form` はその写し（生きた出力との突き合わせは e2e の `cli_help_` の歯）。呼び手は境界 crate の
//! dispatch の 1 か所だけ（[`answer`]）で、pipeline の runner / lens / hook の経路からは呼ばれない。表の字面は NAME を
//! 書かず穴 [`HOLE`] で持ち、描く周に埋める（name-literal）。

use crate::cli_args::HELP_FLAGS;
use crate::cli_outcome::Outcome;
use crate::name::NAME;

/// 表の字面の中で NAME を埋める穴。
const HOLE: &str = "{NAME}";

/// 使い方の行の頭（表の `form` の前に描く周だけ足す）。
const FORM_HEAD: &str = "usage: ";

/// 案内の 1 面（1 command）。
#[derive(Debug)]
pub struct Entry {
    /// 頂点の使い方の 1 行の `<…|…>` の語（`--version` を含む）。
    pub name: &'static str,
    /// 1 文の目的（頂点の一覧と `NAME` の面に出す）。
    pub purpose: &'static str,
    /// 何をするか・人がいつ撃つか・何をしないか（2〜4 文・1 要素 1 行）。
    pub what: &'static [&'static str],
    /// その口の使い方の 1 行の写しから頭の [`FORM_HEAD`] を除いたもの（描く周に頭を足す・引数なしで使い方を出す口は
    /// 足した行が生きた出力の最初の使い方の行と逐語で同じ）。頭を表に書かないのは、pipe の閉包の読み手が頭で始まる
    /// literal を subcommand の使い方の正本として拾うから（写しを第 2 の正本にしない）。
    pub form: &'static str,
    /// 使い方の 1 行の最初の `<…|…>` の語ごとの (語, 1 行の説明)。語を持たない口は `(none)` の 1 行。
    pub subcommands: &'static [(&'static str, &'static str)],
    /// 旗ごとの (旗, 1 行の意味)。旗を持たない口は `(none)` の 1 行。
    pub flags: &'static [(&'static str, &'static str)],
    /// 実在の形の例（1〜3 本）。
    pub examples: &'static [&'static str],
    /// 設計 doc の path（repo の root からの相対・`docs/design/` の下）。
    pub see: &'static [&'static str],
}

/// 語を持たない口の `SUBCOMMANDS` の 1 行。
const NO_SUBCOMMANDS: &[(&str, &str)] = &[("(none)", "This command has no subcommands.")];

/// 旗を持たない口の `FLAGS` の 1 行。
const NO_FLAGS: &[(&str, &str)] = &[("(none)", "This command takes no flags.")];

/// 案内の表（頂点の使い方の 1 行の語の順）。
pub const TABLE: &[Entry] = &[
    Entry {
        name: "name",
        purpose: "Print the name of this vessel.",
        what: &[
            "Prints one line: the vessel name. The binary defines the name in one place only, and",
            "scripts and hooks read this line to tell this vessel apart from others on the host.",
            "It reads no files and no environment, and it ignores any further arguments.",
        ],
        form: "{NAME} name",
        subcommands: NO_SUBCOMMANDS,
        flags: NO_FLAGS,
        examples: &["{NAME} name"],
        see: &["docs/design/core-boundary.md", "docs/design/rules-manifest.md"],
    },
    Entry {
        name: "--version",
        purpose: "Print the version and the commit the binary was built from.",
        what: &[
            "Prints one line: '<name> <version> (<commit>)'. The commit is 12 hex digits, the same",
            "with '+dirty' when the build tree had local changes, or 'unknown' when the build could",
            "not measure it. doctor prints the same line as its second line.",
        ],
        form: "{NAME} --version",
        subcommands: NO_SUBCOMMANDS,
        flags: NO_FLAGS,
        examples: &["{NAME} --version"],
        see: &["docs/design/consumer-sync.md"],
    },
    Entry {
        name: "doctor",
        purpose: "Print a health report of the vessel, the host, its seats and accounts.",
        what: &[
            "With no flags it prints the name, the version and where the host template points.",
            "With --state-dir it adds the repo readiness, the registered seats, the accounts, the",
            "consumers of the plugin and the host guard. With --repo it adds the ledger lines.",
            "It reports what it finds; it repairs nothing.",
        ],
        form: "{NAME} doctor [--state-dir S [--tmux-socket PATH] [--rules FILE]] [--repo R] [flags]",
        subcommands: NO_SUBCOMMANDS,
        flags: &[
            ("--state-dir S", "Host state dir; adds the seat, account, consumer and guard lines."),
            ("--tmux-socket PATH", "tmux socket used to match registered seats against live targets."),
            ("--rules FILE", "Rules manifest read instead of the embedded one."),
            ("--repo R", "Repo root for the init line and the ledger lines (else the cwd)."),
            ("--bin B", "Program resolved for the host guard line (else the vessel name)."),
            ("--unit-dir U", "With --binary PATH: adds the tick unit word to each seat row."),
        ],
        examples: &["{NAME} doctor", "{NAME} doctor --state-dir STATE --repo ."],
        see: &["docs/design/host-init.md", "docs/design/consumer-sync.md"],
    },
    Entry {
        name: "account",
        purpose: "Add, list, retire and restore the accounts that seats run under.",
        what: &[
            "Accounts are declared in the host manifest; each one has its own config dir under the",
            "state dir. This command adds and lists them, retires one (its dir moves aside) or",
            "restores it, and wires the host guard hook into every declared account.",
            "The state dir is always passed with --state-dir; no environment or HOME is read.",
        ],
        form: "account <add <label> [--anchor DIR] [--target T] [--tmux-socket PATH]|ls|retire <label>|restore <label>|wire> --state-dir S",
        subcommands: &[
            ("add", "Add an account under a label, with an optional anchor dir and target."),
            ("ls", "List the accounts and whether each one is ready to run a seat."),
            ("retire", "Retire an account no registered seat uses; its dir moves aside."),
            ("restore", "Bring the latest retired dir of an account back into place."),
            ("wire", "Write the host guard hook into the settings of every declared account."),
        ],
        flags: &[
            ("--state-dir S", "Host state dir (always required)."),
            ("--anchor DIR", "Repo the account is anchored to (add)."),
            ("--target T", "tmux target of the seat (add)."),
            ("--tmux-socket PATH", "tmux socket to use instead of the default one (add)."),
        ],
        examples: &["{NAME} account ls --state-dir STATE", "{NAME} account retire acct2 --state-dir STATE"],
        see: &["docs/design/account-lifecycle.md", "docs/design/account-autonomy.md"],
    },
    Entry {
        name: "rules",
        purpose: "Validate the rules manifest and read one rule from it.",
        what: &[
            "Rules are data: one tracked TOML file with a closed set of kinds. This command checks",
            "that the manifest parses and every row carries its ruling, or prints the value of one",
            "rule. With --state-dir the host manifest is read and checked the same way.",
        ],
        form: "rules <validate|get <id>> [--rules PATH] [--state-dir S]",
        subcommands: &[
            ("validate", "Check the manifest (and the host manifest with --state-dir)."),
            ("get", "Print the value of the rule with the given id."),
        ],
        flags: &[
            ("--rules PATH", "Manifest read instead of the embedded one."),
            ("--state-dir S", "Host state dir; its host.toml is read as well."),
        ],
        examples: &["{NAME} rules validate", "{NAME} rules get R-C4-2"],
        see: &["docs/design/rules-manifest.md"],
    },
    Entry {
        name: "fleet",
        purpose: "Read and append the fleet event log, and measure account usage.",
        what: &[
            "The fleet state is an append-only event log; the current state is its replay.",
            "This command appends one event, shows where one run is, exports the log, measures",
            "or shows how much of each account is left, and picks one account for new work.",
            "Without --state-dir the state dir is resolved from git config, the same way seat does.",
        ],
        form: "fleet <record|show|export|usage|select [--anchor DIR]> [--state-dir D] [flags]",
        subcommands: &[
            ("record", "Append one event (--kind, --run, --bead, --stage and so on)."),
            ("show", "Print the current stage of one run (--run ID)."),
            ("export", "Export the event log for the next vessel generation."),
            ("usage", "Measure or show the remaining usage of each account (--table, --fresh)."),
            ("select", "Pick one account for new work (--anchor DIR)."),
        ],
        flags: &[
            ("--state-dir D", "Host state dir (else resolved from git config)."),
            ("--anchor DIR", "Repo whose seats are left out when picking an account (select)."),
            ("--run ID", "Run to show or to record an event for."),
            ("--account L", "Limit usage to one declared account."),
        ],
        examples: &["{NAME} fleet show --run RUN --state-dir STATE", "{NAME} fleet usage --table --state-dir STATE"],
        see: &["docs/design/fleet-event-log.md", "docs/design/fleet-usage.md"],
    },
    Entry {
        name: "vessel",
        purpose: "Bind a repo to this vessel with the .vessel marker, check it, and update it.",
        what: &[
            "A repo belongs to this vessel when its .vessel marker names it; hooks stay silent in",
            "repos without one. init writes the marker and the state dir into the local git",
            "config, show and check read them back, and update brings the installed vessel up to",
            "the remote branch.",
        ],
        form: "{NAME} vessel <init --state-dir D [--version N]|show|check> [ROOT]",
        subcommands: &[
            ("init", "Write the .vessel marker and the state dir into ROOT (else the cwd)."),
            ("show", "Print the marker of ROOT."),
            ("check", "Exit 0 when ROOT belongs to this vessel, else non-zero."),
            ("update", "Update the installed vessel from --remote R / --branch B."),
        ],
        flags: &[
            ("--state-dir D", "Host state dir recorded in the local git config (init, update)."),
            ("--version N", "Vessel generation written into the marker (init)."),
            ("--remote R", "Remote to update from (update)."),
            ("--branch B", "Branch to update from (update)."),
        ],
        examples: &["{NAME} vessel init --state-dir STATE .", "{NAME} vessel check ."],
        see: &["docs/design/vessel-hook.md", "docs/design/consumer-sync.md"],
    },
    Entry {
        name: "hook",
        purpose: "Answer one Claude Code hook event with the JSON payload read from stdin.",
        what: &[
            "Claude Code runs this command for its hook events; a person rarely types it. It",
            "introduces the seat at session start, guards tool use, stamps the seat busy or idle",
            "and records what it injected. Outside a repo bound to this vessel it prints nothing.",
        ],
        form: "{NAME} hook <event> [--state-dir S] [--pane P] [--rules PATH] [flags] < payload",
        subcommands: &[
            ("session-start", "Introduce the seat and stamp it idle."),
            ("pre-tool-use", "Allow or deny one tool call by the seat's role."),
            ("permission-request", "Answer a permission dialog on an unattended seat."),
            ("user-prompt-submit", "Stamp the seat busy."),
            ("stop", "Stamp the seat idle at the end of a turn."),
            ("pre-compact", "Record the compaction of the seat's context."),
        ],
        flags: &[
            ("--state-dir S", "Host state dir."),
            ("--pane P", "tmux pane of the seat."),
            ("--tmux-socket PATH", "tmux socket of the seat."),
            ("--rules PATH", "Rules manifest read instead of the embedded one."),
            ("--bd PATH", "Ledger program used by the seat."),
        ],
        examples: &["{NAME} hook stop --state-dir STATE < payload.json"],
        see: &["docs/design/vessel-hook.md", "docs/design/seat-state.md", "docs/design/seat-roles.md"],
    },
    Entry {
        name: "host-guard",
        purpose: "Guard the whole host against destructive tool calls, from any repo.",
        what: &[
            "Claude Code runs this command before a tool call, with the JSON payload on stdin.",
            "It denies commands that would destroy state on the host and records each decision",
            "under the state dir. It does not depend on a .vessel marker. 'account wire' installs it.",
        ],
        form: "{NAME} host-guard --state-dir S [--rules PATH] < payload",
        subcommands: NO_SUBCOMMANDS,
        flags: &[
            ("--state-dir S", "Host state dir (required); decisions are recorded there."),
            ("--rules PATH", "Rules manifest read instead of the embedded one."),
        ],
        examples: &["{NAME} host-guard --state-dir STATE < payload.json"],
        see: &["docs/design/vessel-hook.md"],
    },
    Entry {
        name: "pipe",
        purpose: "Run contracts through the pipeline: intake, spawn, gate and land.",
        what: &[
            "A contract is one row of a design doc. The pipeline takes it in, spawns a runner in",
            "its own worktree, gates the result with the verify lines and a lens review, and lands",
            "it on main. Each subcommand moves one run one stage, or shows and steers the queue.",
            "The pipeline has no switch that skips the review stage.",
        ],
        form: "{NAME} pipe <intake|preflight|spawn|approve|answer|gate|land|retire|run|show|resume|stop|report|dispatch> [--state-dir D] [--repo R（cwd は読まない＝--state-dir の無い周と便の写し面の無い周は要る）] [--rules PATH] [stop: --all [--reason WORDS（live な便が 2 本以上の周は要る）]|--run ID] [dispatch: (1 周)|ls|first|hold|release BEAD] [run|resume: --drive] [land: --terminal-only|--detection-only] [--runner CMD] [flags]",
        subcommands: &[
            ("intake", "Take one contract row in and create its run."),
            ("preflight", "Review a contract before intake, without creating a run."),
            ("spawn", "Start the runner for a run in its own worktree."),
            ("approve", "Record the approval a blocked run waits for."),
            ("answer", "Record the answer to the question a run stopped on."),
            ("gate", "Judge a run: verify lines, lens review and the other gates."),
            ("land", "Land a gated run on main and close its ledger entry."),
            ("retire", "Remove the worktree of a finished run."),
            ("run", "Intake a contract and drive it through every stage."),
            ("show", "Print the stage of each run."),
            ("resume", "Continue a stopped run from its current stage."),
            ("stop", "Stop one run (--run ID) or every live run (--all)."),
            ("report", "Summarise finished runs: stages, time and cost."),
            ("dispatch", "Start the next ready contracts, or list, hold and release them."),
            ("land-window", "Wait until nothing is landing, before a merge outside the pipeline."),
            ("regate", "Send a failed gated run back one stage, with the ruling words."),
            ("follow", "Move a live run onto the tip of main without gating it."),
        ],
        flags: &[
            ("--state-dir D", "Host state dir."),
            ("--repo R", "Repo root; the cwd is not read."),
            ("--rules PATH", "Rules manifest read instead of the embedded one."),
            ("--run ID", "The run a subcommand acts on."),
            ("--drive", "Hand the run to the next driver after this stage (run, resume)."),
            ("--runner CMD", "Program that implements a contract."),
            ("--reason WORDS", "Ruling words for stop --all and regate."),
        ],
        examples: &[
            "{NAME} pipe show --state-dir STATE",
            "{NAME} pipe dispatch ls --state-dir STATE --repo .",
            "{NAME} pipe stop --run RUN --state-dir STATE",
        ],
        see: &["docs/design/pipeline.md", "docs/design/dispatcher.md", "docs/design/contract-source.md"],
    },
    Entry {
        name: "runner",
        purpose: "Implement one contract in a worktree with a headless Claude session.",
        what: &[
            "The pipeline starts this command for a run; a person rarely types it. It reads the",
            "contract from stdin, runs Claude in the worktree with the given permission mode and",
            "plugin dir, and ends with a summary line. It does not gate or land the result.",
        ],
        form: "{NAME} runner --worktree D --write-set F --vessel F --plugin-dir D --permission-mode M [--rules PATH] [--account-dir D] [--claude PATH] [--cgroup-root DIR] < contract",
        subcommands: NO_SUBCOMMANDS,
        flags: &[
            ("--worktree D", "Worktree the contract is implemented in."),
            ("--write-set F", "File that lists the paths the runner may change."),
            ("--vessel F", "Vessel file of the run."),
            ("--plugin-dir D", "Plugin dir loaded into the session."),
            ("--permission-mode M", "Permission mode of the session."),
            ("--account-dir D", "Account config dir the session runs under."),
            ("--claude PATH", "Claude program to start instead of the default one."),
            ("--cgroup-root DIR", "cgroup under which the session is confined."),
        ],
        examples: &["{NAME} runner --worktree WT --write-set WS --vessel V --plugin-dir P --permission-mode M < c"],
        see: &["docs/design/pipeline.md", "docs/design/contract-source.md"],
    },
    Entry {
        name: "lens",
        purpose: "Review the diff of one run against its contract with a headless session.",
        what: &[
            "The gate starts this command; a person rarely types it. It reads the diff from stdin,",
            "asks Claude to judge it against the contract, and prints the verdict with its",
            "findings. It changes nothing in the worktree.",
        ],
        form: "{NAME} lens --contract F --worktree D --permission-mode M [--rules PATH] [--account-dir D] [--claude PATH] [--cgroup-root DIR] < diff",
        subcommands: NO_SUBCOMMANDS,
        flags: &[
            ("--contract F", "Contract file the diff is judged against."),
            ("--worktree D", "Worktree of the run."),
            ("--permission-mode M", "Permission mode of the session."),
            ("--account-dir D", "Account config dir the session runs under."),
            ("--claude PATH", "Claude program to start instead of the default one."),
            ("--cgroup-root DIR", "cgroup under which the session is confined."),
        ],
        examples: &["{NAME} lens --contract C --worktree WT --permission-mode M < diff"],
        see: &["docs/design/pipeline.md", "docs/design/gate-cost.md"],
    },
    Entry {
        name: "seat",
        purpose: "Register, launch, tick and retire the tmux seats that run Claude sessions.",
        what: &[
            "A seat is one Claude session in a tmux window, with a role and an account. This",
            "command registers and launches seats, records rulings, runs the management tick and",
            "installs its timer, and retires seats. 'seat <label>' starts a session under that",
            "account in the current terminal.",
        ],
        form: "seat <register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR]|launch --state-dir S --role R --target S:W [--account L] [--anchor DIR] [--model M] [--restore CMD]|ruling add --state-dir S --target T --words W [--bead B] [--rule ID]|ruling ls --state-dir S|tick --state-dir S --target S:W [--rules F]|tick install --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]|tick uninstall --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]|tick status --state-dir S [--target S:W] [--rules F]|retire --state-dir S --target S:W [--reason WORDS]|heartbeat off --state-dir S --target S:W|heartbeat on --state-dir S --target S:W|heartbeat status --state-dir S --target S:W|<label> [--orchestrator] [-c|-r ID] [--target S:W] [--model M] [--anchor DIR] [--restore CMD] [--state-dir S]> [--tmux-socket PATH] [--capture-file PATH] [--state-dir PATH]",
        subcommands: &[
            ("register", "Record a seat row: target, role, account and launch file."),
            ("launch", "Open a tmux window and start a seat with its role."),
            ("ruling add", "Record a ruling that belongs to no run."),
            ("ruling ls", "List the rulings that belong to no run."),
            ("tick", "Run the management tick for one seat once."),
            ("tick install", "Install the timer unit that runs the tick."),
            ("tick uninstall", "Remove the timer unit of the tick."),
            ("tick status", "Print the last tick round and its health for every registered seat."),
            ("retire", "Retire the registered row of a seat."),
            ("heartbeat off", "Stop the tick's heartbeat signal to one seat (the tick keeps running)."),
            ("heartbeat on", "Resume the tick's heartbeat signal to one seat."),
            ("heartbeat status", "Print whether the heartbeat signal of one seat is on or off."),
            ("<label>", "Start a session under account <label> (-c continues, -r ID resumes)."),
        ],
        flags: &[
            ("--state-dir S", "Host state dir."),
            ("--target S:W", "tmux session and window of the seat."),
            ("--role R", "Role of the seat."),
            ("--account L", "Account the seat runs under."),
            ("--model M", "Model of the session."),
            ("--tmux-socket PATH", "tmux socket to use instead of the default one."),
        ],
        examples: &[
            "{NAME} seat launch --state-dir STATE --role planner --target s2:plan",
            "{NAME} seat tick --state-dir STATE --target s2:plan",
        ],
        see: &["docs/design/seat-roles.md", "docs/design/seat-heartbeat.md", "docs/design/account-lifecycle.md"],
    },
    Entry {
        name: "polarity",
        purpose: "Print the polarity list of every guard in the vessel.",
        what: &[
            "Each guard either fails open or fails closed; the list names each guard with its",
            "polarity. CI compares the list with its snapshot. The command reads no arguments,",
            "no stdin and no environment.",
        ],
        form: "{NAME} polarity",
        subcommands: NO_SUBCOMMANDS,
        flags: NO_FLAGS,
        examples: &["{NAME} polarity"],
        see: &["docs/design/polarity.md"],
    },
    Entry {
        name: "contracts",
        purpose: "Check the contract tables of the design docs, or print their schema.",
        what: &[
            "Each design doc may carry a contract table; each row is one contract. check reads",
            "every table under the repo and prints its findings; schema prints the fields a row",
            "may carry. CI runs check on every change.",
        ],
        form: "{NAME} contracts <check --repo R [--rules PATH] [--verbose]|schema>",
        subcommands: &[
            ("check", "Check every contract table under --repo R and print the findings."),
            ("schema", "Print the fields of a contract row."),
        ],
        flags: &[
            ("--repo R", "Repo root whose design docs are read (check)."),
            ("--rules PATH", "Rules manifest read instead of the embedded one."),
            ("--verbose", "Print the measured facts along with the findings."),
        ],
        examples: &["{NAME} contracts check --repo .", "{NAME} contracts schema"],
        see: &["docs/design/contract-source.md"],
    },
];

/// 字面の穴 [`HOLE`] に NAME を埋める。
fn fill(text: &str) -> String {
    text.replace(HOLE, NAME)
}

/// 表を語で引く（無い語は `None`）。
pub fn entry(name: &str) -> Option<&'static Entry> {
    TABLE.iter().find(|found| found.name == name)
}

/// 1 面の `FORM` の行（頭 + 穴を埋めた `form`）。
fn form_line(entry: &Entry) -> String {
    format!("{FORM_HEAD}{}", fill(entry.form))
}

/// `--help` / `-h` の 2 行目（案内への pointer）。
fn pointer(topic: &str) -> String {
    format!("run: {NAME} help {topic}")
}

/// 頂点の案内（語ごとに 1 行の目的と、1 面への案内の 1 行）。
pub fn render_overview() -> Vec<String> {
    let width = TABLE.iter().map(|found| found.name.len()).max().unwrap_or_default();
    let mut lines = vec![fill("{NAME} - AI coding seats and a contract pipeline, run from one binary"), String::new()];
    lines.push("COMMANDS".to_owned());
    lines.extend(TABLE.iter().map(|found| format!("  {:<width$}  {}", found.name, found.purpose)));
    lines.push(String::new());
    lines.push(fill("'{NAME} help <command>' prints the full page of one command."));
    lines
}

/// (語, 説明) の列を幅をそろえた行へ。
fn rows(pairs: &[(&str, &str)]) -> Vec<String> {
    let width = pairs.iter().map(|(word, _)| word.len()).max().unwrap_or_default();
    pairs.iter().map(|(word, text)| format!("  {word:<width$}  {text}")).collect()
}

/// 字下げした行の列。
fn indented(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("  {line}")).collect()
}

/// 1 command の面（NAME / WHAT / FORM / SUBCOMMANDS / FLAGS / EXAMPLES / SEE の順・`FORM` の行は字下げしない写し）。
pub fn render_page(entry: &Entry) -> Vec<String> {
    let name = vec![format!("  {HOLE} {} - {}", entry.name, entry.purpose)];
    let sections = [
        ("NAME", name),
        ("WHAT", indented(entry.what)),
        ("FORM", vec![form_line(entry)]),
        ("SUBCOMMANDS", rows(entry.subcommands)),
        ("FLAGS", rows(entry.flags)),
        ("EXAMPLES", indented(entry.examples)),
        ("SEE", indented(entry.see)),
    ];
    let mut lines = Vec::new();
    for (heading, body) in sections {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(heading.to_owned());
        lines.extend(body.iter().map(|line| fill(line)));
    }
    lines
}

/// 人向けの口の答え（設計 cli-help.md §2 の 1）。`help` は頂点の案内、`help <command>` は 1 面、頂点の `--help` / `-h`
/// は `usage`（頂点の使い方の 1 行）と pointer の 2 行、`<command> --help` / `-h` は `form` と pointer の 2 行（どれも rc 0）。
/// それ以外（`help <未知>` を含む）は `None` で、呼び手の今の経路（未知の引数なら使い方の 1 行・rc 1）がそのまま答える。
pub fn answer(args: &[String], usage: &str) -> Option<Outcome> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["help"] => Some(Outcome::ok(render_overview())),
        ["help", name] => entry(name).map(|found| Outcome::ok(render_page(found))),
        [flag, ..] if HELP_FLAGS.contains(flag) => Some(Outcome::ok(vec![usage.to_owned(), pointer("[<command>]")])),
        [name, flag] if HELP_FLAGS.contains(flag) => entry(name).map(|found| Outcome::ok(vec![form_line(found), pointer(found.name)])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{answer, entry, form_line, render_page, FORM_HEAD, HOLE, TABLE};
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    /// workspace root（この crate の 2 つ上）。
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    /// (f) 表の name は一意（15 語）で、引けば同じ行が返る。
    #[test]
    fn help_table_names_are_unique() {
        let names: BTreeSet<&str> = TABLE.iter().map(|found| found.name).collect();
        assert_eq!(names.len(), TABLE.len(), "name が重なる");
        assert_eq!(TABLE.len(), 15, "頂点の語は 15");
        for found in TABLE {
            assert_eq!(entry(found.name).map(|hit| hit.name), Some(found.name), "{}", found.name);
        }
        assert!(entry("nosuch").is_none(), "未知の語は引けない");
    }

    /// (f) 全欄が非空（文字列の欄は空白だけでもなく、列の欄は 1 要素以上で各要素も非空）で、form は頭を持たず自分の語を名乗る。
    #[test]
    fn help_table_fields_are_not_empty() {
        let blank = |text: &str| text.trim().is_empty();
        for found in TABLE {
            let name = found.name;
            assert!(!blank(found.name) && !blank(found.purpose) && !blank(found.form), "{name}: 文字列の欄が空");
            assert!(!found.form.starts_with(FORM_HEAD), "{name}: form は頭を持たない");
            assert!(found.form.split_whitespace().any(|word| word == name), "{name}: form が自分の語を名乗らない");
            assert!(form_line(found).starts_with(FORM_HEAD), "{name}: 描いた FORM は使い方の行");
            for (label, list) in [("what", found.what), ("examples", found.examples), ("see", found.see)] {
                assert!(!list.is_empty() && !list.iter().any(|line| blank(line)), "{name}: {label} が空");
            }
            for (label, pairs) in [("subcommands", found.subcommands), ("flags", found.flags)] {
                assert!(!pairs.is_empty() && !pairs.iter().any(|(word, text)| blank(word) || blank(text)), "{name}: {label}");
            }
            assert!(found.examples.iter().all(|line| line.starts_with(HOLE)), "{name}: 例は器の名で始まる");
        }
    }

    /// (f) see の path は `docs/design/` の下の `.md` で、repo に実在する。
    #[test]
    fn help_table_see_paths_exist() {
        for found in TABLE {
            for path in found.see {
                assert!(path.starts_with("docs/design/") && path.ends_with(".md"), "{}: {path}", found.name);
                assert!(workspace_root().join(path).is_file(), "{}: {path} が無い", found.name);
            }
        }
    }

    /// 面は見出し 7 つをこの順で持ち、穴は埋まっている（歯の本体は e2e の `cli_help_`）。
    #[test]
    fn help_table_page_carries_headings_in_order() {
        let order = ["NAME", "WHAT", "FORM", "SUBCOMMANDS", "FLAGS", "EXAMPLES", "SEE"];
        for found in TABLE {
            let lines = render_page(found);
            let headings: Vec<&str> = lines.iter().map(String::as_str).filter(|line| order.contains(line)).collect();
            assert_eq!(headings, order, "{}", found.name);
            assert!(!lines.iter().any(|line| line.contains(HOLE)), "{}: 穴が残る", found.name);
        }
        let args = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        assert!(answer(&args(&["help", "nosuch"]), "u").is_none(), "未知の語は呼び手の経路へ");
        assert!(answer(&args(&["pipe", "show"]), "u").is_none(), "案内の口でない引数は呼び手の経路へ");
        assert!(answer(&args(&["ledger", "--help"]), "u").is_none(), "表に無い口の --help は呼び手の経路へ");
    }
}
