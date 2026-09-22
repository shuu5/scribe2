//! run 無しの user 裁定を承認 event として持つ口（設計 docs/design/fleet-event-log.md §9・ADR-0037・SRS FR41 / FR22・憲法 C7.2）。
//!
//! 書き手は `seat ruling add` の 1 本だけで、対話面の席（登録 row の役割が rules 行 [`ID_DIALOGUE_SURFACE`] の値の席）
//! からの逐語を [`EventKind::RulingReceived`]（actor = `human`・`run` 無し）として 1 件書く。ts は器が打ち、それが**裁定 id**
//! になる。読み手は `seat ruling ls`（1 件 1 行）と doctor の突合の 1 行（[`doctor_lines`]・manifest の `user <ts>` の行ごとに
//! 同じ分の event の有無を数えるだけ・判定しない＝C10.2）。

use super::role::role_of_target;
use super::RuleRead;
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli, replay, Event, EventKind, State, ACTOR_HUMAN, SCHEMA};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::collections::BTreeSet;
use std::path::Path;

/// 対話面の役割を宣言する rules 行の id（kind `DialogueSurface`・値は役割の名・ADR-0022 §2.2）。
pub const ID_DIALOGUE_SURFACE: &str = "R-C7-1";

/// 裁定を書かずに断る理由（閉じた enum・断る周は event を 1 byte も書かない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulingRefusal {
    /// 逐語が空（空白だけも空）＝聞いた形だけが残る記録を作らない（`pipe approve` の `record_words` と同じ型）。
    EmptyWords,
    /// target の登録 row の役割が対話面の行の値でない・row が無い・log か行を読めない（FailClosed）。
    NotDialogueSurface,
    /// event log へ書けない（理由の本文）。
    Store(String),
}

impl RulingRefusal {
    /// 行に出す字面（store の断りは理由の本文を添える）。
    pub fn render(&self, target: &str) -> String {
        let reason = match self {
            Self::EmptyWords => "empty-words".to_owned(),
            Self::NotDialogueSurface => "not-dialogue-surface".to_owned(),
            Self::Store(text) => format!("store detail={text}"),
        };
        format!("seat ruling: refused reason={reason} target={target}")
    }
}

/// target が対話面の席か（pure）: 登録 row の役割（[`role_of_target`]・役割の解決の 1 本）が、発効した行
/// [`ID_DIALOGUE_SURFACE`] の値と一致する周だけ `Ok`。row が無い・行が無い / 不発効 / 文字列でない・manifest を読めない周は
/// [`RulingRefusal::NotDialogueSurface`]（読めなさを「対話面」に倒さない）。
pub fn surface_check(state: &State, target: &str, manifest: &Result<Manifest, RuleRead>) -> Result<(), RulingRefusal> {
    let role = role_of_target(state, target).ok_or(RulingRefusal::NotDialogueSurface)?;
    let row = manifest.as_ref().ok().and_then(|found| found.get(ID_DIALOGUE_SURFACE)).filter(|row| row.enabled);
    match row.map(|found| &found.value) {
        Some(RuleValue::Str(name)) if name == role.as_str() => Ok(()),
        _ => Err(RulingRefusal::NotDialogueSurface),
    }
}

/// 裁定 1 件の材料（`seat ruling add` の引数）。
pub struct Draft<'a> {
    /// 対話面の席の target（`session:window`）。
    pub target: &'a str,
    /// user の逐語（要約しない・言い換えた時点で裁定ではなくなる）。
    pub words: &'a str,
    /// 裁定が指す契約の bead id（任意）。
    pub bead: Option<&'a str>,
    /// 裁定が指す rules 行の id（任意）。
    pub rule: Option<&'a str>,
}

/// 裁定を 1 件書く（設計 §9 (2)）。**逐語を先に測り**、次に対話面の席かを測る（断る周は event を書かない）。manifest は
/// 埋め込み（tracked）の 1 本（席の口は `--rules` を受けない）。書いた event を返す（ts が裁定 id）。
pub fn add(state_dir: &Path, draft: &Draft<'_>) -> Result<Event, RulingRefusal> {
    if draft.words.trim().is_empty() {
        return Err(RulingRefusal::EmptyWords);
    }
    let state = store::read_all(state_dir).map(|events| replay(&events)).map_err(|_| RulingRefusal::NotDialogueSurface)?;
    surface_check(&state, draft.target, &super::embedded_manifest())?;
    let event = Event {
        schema: SCHEMA,
        ts: cli::now_utc(),
        kind: EventKind::RulingReceived,
        run: String::new(),
        bead: draft.bead.unwrap_or_default().to_owned(),
        host: cli::host(),
        actor: ACTOR_HUMAN.to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(draft.words.to_owned()),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: draft.rule.map(str::to_owned),
    };
    let store_err = |err: store::StoreError| RulingRefusal::Store(err.to_string());
    store::append(state_dir, &event, LockPolicy::embedded().map_err(store_err)?).map_err(store_err)?;
    Ok(event)
}

/// 裁定 1 件の 1 行（pure・`ruling: ts=<ts> bead=<b> rule=<id> words=<逐語>`）。無い欄は `-`、逐語は改行や `"` を含んでも 1 行に
/// 収まるよう引用符つきで escape する（中身は逐語のまま・要約しない）。
pub fn render(event: &Event) -> String {
    let or_dash = |text: &str| if text.is_empty() { "-".to_owned() } else { text.to_owned() };
    format!(
        "ruling: ts={} bead={} rule={} words={:?}",
        event.ts,
        or_dash(&event.bead),
        or_dash(event.rule.as_deref().unwrap_or_default()),
        event.detail.as_deref().unwrap_or_default()
    )
}

/// log の裁定の一覧（物理順・1 件 1 行）。読めない log は `Err`（0 件に潰さない）。
pub fn ls(state_dir: &Path) -> Result<Vec<String>, Vec<store::StoreError>> {
    let events = store::read_all(state_dir)?;
    Ok(events.iter().filter(|event| event.kind == EventKind::RulingReceived).map(render).collect())
}

/// manifest の行の `ruling` 欄が指す裁定の分（`user <YYYY-MM-DDTHH:MMZ>` の形の先頭の語・pure）。
///
/// `user ` で始まらない行は `None`（母集団の外）、`user ` で始まるが分まで一意に読めない形（`4xZ`・日付だけ・散文）は
/// `Some(None)`（`skipped` に数える）、読める行は `Some(Some("YYYY-MM-DDTHH:MM"))`。
pub fn ruling_minute(ruling: &str) -> Option<Option<String>> {
    let rest = ruling.strip_prefix("user ")?;
    let token = rest.split_whitespace().next().unwrap_or_default();
    let minute = token.strip_suffix('Z').filter(|body| {
        let bytes = body.as_bytes();
        bytes.len() == 16
            && bytes.iter().enumerate().all(|(at, byte)| match at {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 => *byte == b':',
                _ => byte.is_ascii_digit(),
            })
    });
    Some(minute.map(str::to_owned))
}

/// doctor の突合の数（pure・[`Tally::line`] の材料）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    /// log の裁定の数。
    pub rulings: usize,
    /// 母集団（`user <分>` の行）のうち同じ分の裁定が在る行の数。
    pub matched: usize,
    /// 母集団の行の数。
    pub of: usize,
    /// 同じ分の裁定が無い行の id（manifest の順）。
    pub unmatched: Vec<String>,
    /// `user ` で始まるが分まで読めない行の数（母集団の外）。
    pub skipped: usize,
}

impl Tally {
    /// log と manifest から数える（pure）。同じ分に裁定が複数在る行も matched 1 行に数える（1 行が 1 件を一意に指すことは保証しない）。
    pub fn of(events: &[Event], manifest: &Manifest) -> Self {
        let rulings: Vec<&Event> = events.iter().filter(|event| event.kind == EventKind::RulingReceived).collect();
        let minutes: BTreeSet<&str> = rulings.iter().filter_map(|event| event.ts.get(..16)).collect();
        let (mut matched, mut of, mut skipped, mut unmatched) = (0_usize, 0_usize, 0_usize, Vec::new());
        for row in manifest.rows() {
            match ruling_minute(&row.ruling) {
                None => {}
                Some(None) => skipped = skipped.saturating_add(1),
                Some(Some(minute)) => {
                    of = of.saturating_add(1);
                    if minutes.contains(minute.as_str()) {
                        matched = matched.saturating_add(1);
                    } else {
                        unmatched.push(row.id.clone());
                    }
                }
            }
        }
        Self { rulings: rulings.len(), matched, of, unmatched, skipped }
    }

    /// doctor の 1 行（`rulings=<n> rule-rulings=<matched>/<of> unmatched=<id,…|-> skipped=<n>`）。
    pub fn line(&self) -> String {
        let unmatched = if self.unmatched.is_empty() { "-".to_owned() } else { self.unmatched.join(",") };
        format!(
            "rulings={} rule-rulings={}/{} unmatched={unmatched} skipped={}",
            self.rulings, self.matched, self.of, self.skipped
        )
    }

    /// 数えるものが何も無い周か（裁定 0・母集団 0・skipped 0）。
    fn is_empty(&self) -> bool {
        self.rulings == 0 && self.of == 0 && self.skipped == 0
    }
}

/// doctor の突合の行（設計 §9 (4)・読むだけ・判定しない＝rc を変えない）。manifest は `rules`（`--rules` の値）か埋め込み。
///
/// 裁定 0・母集団 0・skipped 0 の周は**行を出さない**（数えるものの無い置き場の doctor の外形を動かさない）。log か manifest を
/// 読めない周は数を 0 と書かず `unreadable` を名乗る 1 行を出す。
pub fn doctor_lines(state_dir: &Path, rules: Option<&str>) -> Vec<String> {
    let manifest = super::manifest_read(rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))));
    match (store::read_all(state_dir), manifest) {
        (Ok(events), Ok(found)) => {
            let tally = Tally::of(&events, &found);
            if tally.is_empty() {
                Vec::new()
            } else {
                vec![tally.line()]
            }
        }
        (Err(_), _) => vec!["rulings=unreadable rule-rulings=unmeasurable".to_owned()],
        (Ok(events), Err(_)) => {
            let rulings = events.iter().filter(|event| event.kind == EventKind::RulingReceived).count();
            vec![format!("rulings={rulings} rule-rulings=manifest-unreadable")]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ruling_minute, surface_check, RulingRefusal, Tally};
    use crate::fleet::{Event, EventKind, Registration, State, ACTOR_HUMAN, SCHEMA};
    use crate::rules::manifest::Manifest;
    use crate::seat::role::Role;
    use crate::seat::RuleRead;

    /// `[[rule]]` の行の本文（id・ruling・発効）。
    fn row(id: &str, kind: &str, value: &str, enabled: bool, ruling: &str) -> String {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = {enabled}\nruling = \"{ruling}\"\nruled_at = \"d\"\n")
    }

    /// 本文から manifest を読む。
    fn manifest(rows: &str) -> Manifest {
        match Manifest::parse(&format!("schema = 1\n{rows}")) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        }
    }

    /// target `s:w` に登録 row を 1 件持つ state。
    fn registered() -> State {
        let registration = Registration {
            role: Role::Orchestrator,
            anchor: "/repo".to_owned(),
            target: "s:w".to_owned(),
            sid: None,
            account: "a".to_owned(),
            launch: "l".to_owned(),
            model: None,
        };
        let event = Event {
            schema: SCHEMA,
            ts: "2026-09-22T00:00:00Z".to_owned(),
            kind: EventKind::SeatRegistered,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: "machine".to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: None,
            registration: Some(registration),
            mark: None,
            account: None,
            cost: None,
            rule: None,
        };
        crate::fleet::replay(&[event])
    }

    /// 裁定 1 件（ts だけを選ぶ）。
    fn ruling(ts: &str) -> Event {
        Event {
            schema: SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::RulingReceived,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: ACTOR_HUMAN.to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: Some("推奨で".to_owned()),
            allowance: None,
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
        }
    }

    /// 対話面の判定（pure）: 行の値と登録 row の役割が一致する周だけ `Ok`。row が無い target・行が無い / 不発効・manifest を
    /// 読めない周はどれも `NotDialogueSurface`（FailClosed・読めなさを対話面に倒さない）。
    #[test]
    fn fleet_ruling_surface_check_is_fail_closed_on_every_unreadable_or_foreign_case() {
        let state = registered();
        let on = Ok(manifest(&row("R-C7-1", "DialogueSurface", "\"orchestrator\"", true, "r")));
        assert_eq!(surface_check(&state, "s:w", &on), Ok(()), "対話面の役割の row");
        let refused = Err(RulingRefusal::NotDialogueSurface);
        assert_eq!(surface_check(&state, "other:w", &on), refused, "row の無い target");
        assert_eq!(surface_check(&State::default(), "s:w", &on), refused, "登録の無い log");
        let off = Ok(manifest(&row("R-C7-1", "DialogueSurface", "\"orchestrator\"", false, "r")));
        assert_eq!(surface_check(&state, "s:w", &off), refused, "不発効の行");
        assert_eq!(surface_check(&state, "s:w", &Ok(manifest(""))), refused, "行が無い");
        assert_eq!(surface_check(&state, "s:w", &Err(RuleRead::ManifestUnreadable)), refused, "manifest を読めない");
    }

    /// `ruling` 欄の読み（pure）: `user <分>Z` は分、`user ` で始まるが分まで読めない形は `Some(None)`（skipped）、`user ` で
    /// 始まらない行は母集団の外（`None`）。
    #[test]
    fn fleet_ruling_minute_reads_only_minute_precise_user_rulings() {
        assert_eq!(ruling_minute("user 2026-09-17T07:30Z"), Some(Some("2026-09-17T07:30".to_owned())));
        assert_eq!(ruling_minute("user 2026-09-14T13:23Z bats in runner"), Some(Some("2026-09-14T13:23".to_owned())), "後ろの散文は読まない");
        for vague in ["user 2026-09-15T11:2xZ", "user 2026-09-14", "user 裁定 2026-09-10", "user 2026-09-17T07:30", "user "] {
            assert_eq!(ruling_minute(vague), Some(None), "{vague} は分まで読めない");
        }
        for outside in ["RULING-v2 論点 2", "grill U3", "r", "users 2026-09-17T07:30Z"] {
            assert_eq!(ruling_minute(outside), None, "{outside} は母集団の外");
        }
    }

    /// 突合（pure）: 同じ分の裁定が在る行は matched・無い行は id で名指し・分の曖昧な行は skipped・同じ分に 2 件在っても 1 行。
    #[test]
    fn fleet_ruling_tally_matches_rows_by_the_same_minute() {
        let rows = [
            row("a.one", "CoreLines", "1", true, "user 2026-09-17T07:30Z"),
            row("b.two", "CoreLines", "1", true, "user 2026-09-18T01:02Z"),
            row("c.vague", "CoreLines", "1", true, "user 2026-09-15T11:2xZ"),
            row("d.other", "CoreLines", "1", true, "grill U3"),
        ]
        .concat();
        let events = [ruling("2026-09-17T07:30:59Z"), ruling("2026-09-17T07:30:01Z"), ruling("2026-09-18T01:03:00Z")];
        let tally = Tally::of(&events, &manifest(&rows));
        assert_eq!(tally, Tally { rulings: 3, matched: 1, of: 2, unmatched: vec!["b.two".to_owned()], skipped: 1 });
        assert_eq!(tally.line(), "rulings=3 rule-rulings=1/2 unmatched=b.two skipped=1");
        let none = Tally::of(&[], &manifest(""));
        assert!(none.is_empty(), "数えるものの無い周");
        assert_eq!(Tally::of(&events, &manifest("")).line(), "rulings=3 rule-rulings=0/0 unmatched=- skipped=0");
    }
}
