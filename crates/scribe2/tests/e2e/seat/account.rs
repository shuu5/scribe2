//! 口座の歯（doctor の口座の行 / 口座の退避と立て直し / hook 集合の食い違いの後の終了の手と立て直し・設計
//! docs/design/seat-roles.md §7 / account-autonomy.md §5 / account-lifecycle.md §8・接頭辞 `doctor_accounts_` /
//! `seat_account_` / `seat_tick_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat.rs` から**挙動不変で移した**もの（`s2-07l.261`）。状態 / 役割 / 登録の歯は `register`・起動 /
//! 復元 / Enter 落ちの歯は `launch`・rules の歯は `rules` へ**挙動不変で移した**（`s2-07l.361`・seat-roles.md §7）。
// flip-check: moved s2-07l.261
// flip-check: moved s2-07l.361

use super::*;

// ─────────────────────────── doctor の口座の行（s2-07l.233・account-autonomy.md §5） ───────────────────────────

/// `<state>/accounts/<label>` を dir で作り、直下に `files`（名前・本文）を置く。
fn account_fixture(place: &RolePlace, label: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = place.state.join("accounts").join(label);
    fs::create_dir_all(&dir).ok();
    for (name, body) in files {
        fs::write(dir.join(name), body).ok();
    }
    dir
}

/// 行の列のうち `account=<label> ` で始まる 1 行（無ければ空）。
fn account_line(lines: &[String], label: &str) -> String {
    let head = format!("account={label} ");
    lines.iter().find(|line| line.starts_with(&head)).cloned().unwrap_or_default()
}

/// `path` から下の全 entry の (path, 本文, mtime)（path の順）。dir の本文は空。
fn tree_facts(path: &Path) -> Vec<(PathBuf, Vec<u8>, Option<SystemTime>)> {
    let mtime = fs::metadata(path).and_then(|found| found.modified()).ok();
    let mut facts = vec![(path.to_path_buf(), fs::read(path).unwrap_or_default(), mtime)];
    let mut children: Vec<PathBuf> =
        fs::read_dir(path).map(|entries| entries.filter_map(|entry| entry.ok().map(|found| found.path())).collect()).unwrap_or_default();
    children.sort();
    for child in children {
        facts.extend(tree_facts(&child));
    }
    facts
}

/// credential だけの dir と設定 dir 全体（`settings.json` を持つ・link で置く周も）で `config=` が分かれ、
/// `agentview=` は `disableAgentView` を読む（歯 (a)・flip の RED＝base は口座の行を出さない）。
#[test]
fn doctor_accounts_config_splits_credential_only_dir_from_full_config_dir() {
    let place = role_place();
    account_fixture(&place, "cred-only", &[(".credentials.json", "{}")]);
    account_fixture(&place, "full-off", &[(".credentials.json", "{}"), ("settings.json", "{\"disableAgentView\": true}")]);
    account_fixture(&place, "full-on", &[(".credentials.json", "{}"), ("settings.json", "{\"disableAgentView\": false}")]);
    account_fixture(&place, "full-bare", &[("settings.json", "{}")]);
    account_fixture(&place, "full-broken", &[("settings.json", "{\"disableAgentView\": tru")]);
    account_fixture(&place, "full-shape", &[("settings.json", "{\"disableAgentView\": \"yes\"}")]);
    let real = place.dir.join("config-home");
    fs::create_dir_all(&real).ok();
    fs::write(real.join(".credentials.json"), "{}").ok();
    fs::write(real.join("settings.json"), "{\"disableAgentView\": true}").ok();
    std::os::unix::fs::symlink(&real, place.state.join("accounts").join("linked")).expect("link を置ける");
    let labels = ["cred-only", "full-off", "full-on", "full-bare", "full-broken", "full-shape", "linked", "gone"];
    let lines = doctor_rows(&place, &account_rules(&labels));
    for (label, rest) in [
        ("cred-only", "dir=present credential=present config=missing agentview=unreadable"),
        ("full-off", "dir=present credential=present config=present agentview=off"),
        ("full-on", "dir=present credential=present config=present agentview=on"),
        ("full-bare", "dir=present credential=missing config=present agentview=on"),
        ("full-broken", "dir=present credential=missing config=present agentview=unreadable"),
        ("full-shape", "dir=present credential=missing config=present agentview=unreadable"),
        ("linked", "dir=present credential=present config=present agentview=off"),
        ("gone", "dir=missing credential=missing config=missing agentview=unreadable"),
    ] {
        assert_eq!(account_line(&lines, label), format!("account={label} {rest} trust=n/a retired=no"), "{lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` で `trust=` が分かれ、file が無い・壊れた・形が違う
/// 周は `unreadable`（`missing` に潰さない）。別の anchor の key は数えない（歯 (b)）。
#[test]
fn doctor_accounts_trust_reads_the_anchor_key_and_never_folds_unreadable_into_missing() {
    let place = role_doctor_place();
    let key = |value: &str| format!("{{\"projects\":{{\"/repo\":{{\"hasTrustDialogAccepted\":{value}}}}}}}");
    let cases = [
        ("t-true", Some(key("true")), "accepted"),
        ("t-absent", Some("{\"projects\":{\"/repo\":{}}}".to_owned()), "missing"),
        ("t-false", Some(key("false")), "missing"),
        ("t-nofile", None, "unreadable"),
        ("t-broken", Some("{\"projects\":".to_owned()), "unreadable"),
        ("t-other", Some("{\"projects\":{\"/elsewhere\":{\"hasTrustDialogAccepted\":true}}}".to_owned()), "missing"),
        ("t-noproj", Some("{}".to_owned()), "missing"),
        ("t-shape", Some("{\"projects\":[]}".to_owned()), "unreadable"),
        ("t-string", Some(key("\"true\"")), "unreadable"),
    ];
    for (label, body, _) in &cases {
        let files: Vec<(&str, &str)> = body.iter().map(|found| (".claude.json", found.as_str())).collect();
        account_fixture(&place, label, &files);
    }
    let labels: Vec<&str> = cases.iter().map(|(label, _, _)| *label).collect();
    let lines = doctor_rows(&place, &account_rules(&labels));
    for (label, _, value) in &cases {
        let want = format!("account={label} dir=present credential=missing config=missing agentview=unreadable trust={value} retired=no");
        assert_eq!(account_line(&lines, label), want, "{lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// 登録 row 0 件は `trust=n/a`・anchor が複数なら anchor ごとに `trust=<潰した anchor>:<値>`（辞書順）・event log を
/// 読めない周は `trust=unreadable`（n/a に潰さない）（歯 (c)）。
#[test]
fn doctor_accounts_trust_is_na_without_rows_and_per_anchor_with_many() {
    let place = role_place();
    let body = "{\"projects\":{\"/repo/a\":{\"hasTrustDialogAccepted\":true},\"/repo/b\":{}}}";
    account_fixture(&place, "multi", &[(".claude.json", body)]);
    let rules = account_rules(&["multi"]);
    let head = "account=multi dir=present credential=missing config=missing agentview=unreadable";
    assert_eq!(account_line(&doctor_rows(&place, &rules), "multi"), format!("{head} trust=n/a retired=no"), "登録 row 0 件");
    for (target, role, anchor) in [("mb:x", "orchestrator", "/repo/b"), ("ma:x", "orchestrator", "/repo/a"), ("mc:x", "orchestrator", "/repo/b")] {
        role_stamp(&place, target, Some("sid-t"));
        let out = role_register(&place, target, role, &["--anchor", anchor]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let lines = doctor_rows(&place, &rules);
    assert_eq!(account_line(&lines, "multi"), format!("{head} trust=_repo_a:accepted trust=_repo_b:missing retired=no"), "{lines:?}");
    fs::write(vessel::fleet::store::events_path(&place.state), "not an event\n").expect("log を壊せる");
    let lines = doctor_rows(&place, &rules);
    // log を読めない周は退役も読めない（`no` に潰さない・C11）。
    assert_eq!(account_line(&lines, "multi"), format!("{head} trust=unreadable retired=unreadable"), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("seats: registered=unreadable")), "{lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// 口座の行は突合の行の直後に label の辞書順で並ぶ（宣言順ではない）（歯 (d)）。
#[test]
fn doctor_accounts_lines_follow_the_seat_lines_in_label_order() {
    let place = role_doctor_place();
    let lines = doctor_rows(&place, &account_rules(&["zeta", "alpha", "mid"]));
    let labels: Vec<&str> =
        lines.iter().filter_map(|line| line.strip_prefix("account=")).filter_map(|rest| rest.split(' ').next()).collect();
    assert_eq!(labels, ["alpha", "mid", "zeta"], "{lines:?}");
    let seats = lines.iter().position(|line| line.starts_with("seats: "));
    let host = lines.iter().position(|line| line == HOST_ABSENT);
    let first = lines.iter().position(|line| line.starts_with("account="));
    assert_eq!(host, seats.map(|at| at + 1), "host の面の行は突合の行の直後: {lines:?}");
    assert_eq!(first, seats.map(|at| at + 2), "口座の行は host の面の行の直後: {lines:?}");
    assert_eq!(lines.len(), 9, "2 行 + 登録 row 1 行 + 突合 1 行 + host の面 1 行 + 口座 3 行 + 導入先 1 行: {lines:?}");
    assert_eq!(lines.last().map(String::as_str), Some(CONSUMER_REPO), "導入先の行は口座の行の後ろ: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// `[[account]]` の無い manifest は口座の行 0 本で他の行は不変・`--rules` 無しは host の面（置き場の `host.toml`）の宣言・
/// 宣言なしなら行 0・読めない manifest は 1 行で名乗り rc は変えない・`--rules` の誤りは使い方で断る（歯 (e)・
/// 埋め込みの宣言に依らない形＝`s2-07l.243` の裁定 (B)）。
#[test]
fn doctor_accounts_no_declared_account_adds_no_line_and_keeps_the_rest() {
    let place = role_doctor_place();
    let without = doctor_rows(&place, NO_ACCOUNT_RULES);
    let with = doctor_rows(&place, &account_rules(&["solo"]));
    assert!(!without.iter().any(|line| line.starts_with("account=")), "{without:?}");
    assert_eq!(without.len(), 6, "2 行 + 登録 row 1 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行: {without:?}");
    let rest: Vec<String> = with.iter().filter(|line| !line.starts_with("account=")).cloned().collect();
    assert_eq!(rest, without, "他の行は不変");
    assert_eq!(with.len(), without.len() + 1, "{with:?}");
    let state = place.state.display().to_string();
    let doctor = |args: &[&str]| Command::new(bin()).arg("doctor").args(args).output().expect("binary を起動できる");
    let labels_of = |out: &Output| -> Vec<String> {
        stdout_of(out)
            .lines()
            .filter_map(|line| line.strip_prefix("account=").and_then(|rest| rest.split(' ').next()).map(str::to_owned))
            .collect()
    };
    let bare = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket]);
    assert_eq!(rc_of(&bare), i32::from(RC_OK), "stderr={}", stderr_of(&bare));
    assert_eq!(labels_of(&bare), Vec::<String>::new(), "--rules 無し・host の面も無い周は口座の行 0");
    let bare_out = stdout_of(&bare);
    let bare_tail: Vec<&str> = bare_out.lines().rev().take(2).collect();
    assert_eq!(bare_tail, [CONSUMER_REPO, HOST_ABSENT], "口座の行 0 でも導入先の行は出る: {bare_out}");
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), account_rules(&["zhost", "ahost"])).expect("host の面を書ける");
    let hosted = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket]);
    assert_eq!(rc_of(&hosted), i32::from(RC_OK), "stderr={}", stderr_of(&hosted));
    assert_eq!(labels_of(&hosted), ["ahost", "zhost"], "--rules 無しは host の面の label の辞書順");
    fs::remove_file(place.state.join(vessel::rules::HOST_MANIFEST)).expect("host の面を外せる");
    let absent = place.dir.join("no-such-rules.toml").display().to_string();
    let unreadable = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket, "--rules", &absent]);
    assert_eq!(rc_of(&unreadable), i32::from(RC_OK), "rc は変えない");
    assert_eq!(stdout_of(&unreadable).lines().last(), Some("accounts: manifest=unreadable"), "0 行に潰さない");
    let rules = fixture(&place.dir, "solo.toml", &account_rules(&["solo"]));
    for bad in [
        &["--rules", &rules][..],
        &["--state-dir", &state, "--rules", ""],
        &["--state-dir", &state, "--rules"],
        &["--state-dir", &state, "--rules", &rules, "--rules", &rules],
    ] {
        let out = doctor(bad);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{bad:?} は使い方で断る");
        let stdout = stdout_of(&out);
        assert!(stdout.starts_with("usage: ") && !stdout.contains("account="), "{bad:?}: {stdout}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// doctor は口座の dir に何も書かない（全 entry の本文・mtime が不変・無い口座の dir を作らない）（歯 (f)）。
#[test]
fn doctor_accounts_writes_nothing_into_the_account_dirs() {
    let place = role_doctor_place();
    let full = [(".credentials.json", "{\"k\":1}"), ("settings.json", "{\"disableAgentView\": true}"), (".claude.json", "{\"projects\":{}}")];
    account_fixture(&place, "w-full", &full);
    account_fixture(&place, "w-empty", &[]);
    let accounts = place.state.join("accounts");
    let before = tree_facts(&accounts);
    assert_eq!(before.len(), 6, "accounts + dir 2 つ + file 3 つ: {before:?}");
    sleep(Duration::from_millis(20));
    let lines = doctor_rows(&place, &account_rules(&["w-full", "w-empty", "w-gone"]));
    assert_eq!(lines.iter().filter(|line| line.starts_with("account=")).count(), 3, "{lines:?}");
    assert_eq!(tree_facts(&accounts), before, "本文・mtime が不変");
    assert!(!accounts.join("w-gone").exists(), "無い口座の dir を作らない");
    fs::remove_dir_all(&place.dir).ok();
}

// ─── doctor の群の行（account-lifecycle.md §17 の約束 7 / 8・ADR-0049・接頭辞 `host_group_`） ───

/// 置き場の host の面に群を宣言する（口座の表は持たない＝候補は `--rules` の tracked の面の label を指す）。
/// `groups` は (名, 置き場の列, 候補の口座の列) の宣言順。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_groups(place: &RolePlace, groups: &[(&str, &[&str], &[&str])]) {
    let quoted = |items: &[&str]| items.iter().map(|item| format!("\"{item}\"")).collect::<Vec<String>>().join(", ");
    let body = groups.iter().fold("schema = 1\n".to_owned(), |body, (name, anchors, accounts)| {
        format!(
            "{body}\n[[account-group]]\nname = \"{name}\"\nanchors = [{}]\naccounts = [{}]\n",
            quoted(anchors),
            quoted(accounts)
        )
    });
    fs::create_dir_all(&place.state).expect("置き場を作れる");
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), body).expect("host の面を書ける");
}

/// 行の列のうち `group=<name> ` で始まる 1 行（無ければ空）。
fn group_line(lines: &[String], name: &str) -> String {
    let head = format!("group={name} ");
    lines.iter().find(|line| line.starts_with(&head)).cloned().unwrap_or_default()
}

/// (a) 宣言された群 1 つにつき 1 行が**宣言順**（label の昇順ではない）で、口座の行の後ろ・導入先の行の前に並ぶ。
/// 項目は名・候補の label の列（宣言順）・置き場の数・その群の置き場の席の登録 row の口座 label（`role_doctor_place`
/// の row は anchor `/repo` = `acct-1`）。base は群の行を 1 本も出さない（RED）。
#[test]
fn host_group_doctor_prints_one_line_per_group_in_declaration_order() {
    let place = role_doctor_place();
    put_groups(&place, &[("zeta", &["/repo", "/repo/b"], &["acct-1", "spare"]), ("alpha", &["/repo/c"], &["spare"])]);
    let lines = doctor_rows(&place, &account_rules(&["acct-1", "spare"]));
    let names: Vec<&str> =
        lines.iter().filter_map(|line| line.strip_prefix("group=")).filter_map(|rest| rest.split(' ').next()).collect();
    assert_eq!(names, ["zeta", "alpha"], "宣言順（辞書順ではない）: {lines:?}");
    assert_eq!(
        group_line(&lines, "zeta"),
        "group=zeta accounts=acct-1,spare anchors=2 seat-accounts=acct-1",
        "候補は宣言順・置き場は数・席の口座は登録 row から: {lines:?}"
    );
    let last_account = lines.iter().rposition(|line| line.starts_with("account="));
    let first_group = lines.iter().position(|line| line.starts_with("group="));
    assert_eq!(first_group, last_account.map(|at| at + 1), "群の行は口座の行の後ろ: {lines:?}");
    assert_eq!(lines.last().map(String::as_str), Some(CONSUMER_REPO), "導入先の行は群の行の後ろ: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) その群の置き場に席の登録 row が 1 つも無い周は無しの語（`none`）で、`0` や空に潰さない。置き場の一致は登録が
/// 書いた値そのもの（`/repo` の末尾 `/` 違いはどの row とも一致しない）。
#[test]
fn host_group_doctor_line_says_none_without_seat_rows() {
    let place = role_doctor_place();
    put_groups(&place, &[("alpha", &["/repo/elsewhere"], &["acct-1"]), ("beta", &["/repo/"], &["acct-1"])]);
    let lines = doctor_rows(&place, &account_rules(&["acct-1"]));
    assert_eq!(group_line(&lines, "alpha"), "group=alpha accounts=acct-1 anchors=1 seat-accounts=none", "{lines:?}");
    assert_eq!(group_line(&lines, "beta"), "group=beta accounts=acct-1 anchors=1 seat-accounts=none", "正規化しない: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) 置き場を 2 つ持つ群は両方の登録 row の口座を畳んで（重複は 1 つ・辞書順で）載せる。event log を読めない周は
/// `unreadable`（`none` に潰さない・C11）。行は判定しない（rc 0 のまま）。
#[test]
fn host_group_doctor_line_lists_the_seat_accounts_of_the_group_anchors() {
    let place = role_doctor_place();
    role_register_extra(&place, "grpb:grpb", "/repo/b");
    put_groups(&place, &[("alpha", &["/repo", "/repo/b"], &["acct-1"])]);
    let rules = account_rules(&["acct-1"]);
    assert_eq!(
        group_line(&doctor_rows(&place, &rules), "alpha"),
        "group=alpha accounts=acct-1 anchors=2 seat-accounts=acct-1",
        "2 つの row は同じ口座＝畳んで 1 つ"
    );
    fs::write(vessel::fleet::store::events_path(&place.state), "not an event\n").expect("log を壊せる");
    assert_eq!(
        group_line(&doctor_rows(&place, &rules), "alpha"),
        "group=alpha accounts=acct-1 anchors=2 seat-accounts=unreadable",
        "読めなさを none に潰さない"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 群を 1 つも宣言しない host は群の行が 0 本で、doctor の既存の外形は 1 行も動かない（host の面が無い周も、
/// 群を持たない `[[account]]` だけの host の面が在る周も、行の列が群なしの周と一致する）。
#[test]
fn host_group_doctor_prints_no_line_without_groups() {
    let place = role_doctor_place();
    let rules = account_rules(&["acct-1"]);
    let bare = doctor_rows(&place, &rules);
    assert!(!bare.iter().any(|line| line.starts_with("group=")), "群の行 0 本: {bare:?}");
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), "schema = 1\n\n[[account]]\nlabel = \"hosted\"\n")
        .expect("host の面を書ける");
    let hosted = doctor_rows(&place, &rules);
    assert!(!hosted.iter().any(|line| line.starts_with("group=")), "群を持たない面でも 0 本: {hosted:?}");
    // 比べるのは群の行の入る隙間（口座の行と host の面の 3 値の行を除いた外形）。
    let shape = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter(|line| !line.starts_with("account=") && !line.starts_with("host-manifest="))
            .cloned()
            .collect()
    };
    assert_eq!(shape(&hosted), shape(&bare), "口座の行と host の面の行の外は 1 行も動かない");
    assert!(bare.contains(&HOST_ABSENT.to_owned()), "面の無い周の 1 行は従来どおり: {bare:?}");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 口座の退避と立て直し（account-autonomy.md §5・`s2-07l.211`・接頭辞 `seat_account_`） ───────────────────

// ─────────────────── hook 集合の食い違いの後の終了の手と立て直し（consumer-sync.md §6・AC32・`s2-07l.304`・接頭辞 `seat_tick_hook_drift_`） ───────────────────

// ─────────────────── 立て直しと row の model（`s2-07l.313`・接頭辞 `seat_account_`） ───────────────────
