//! lens の findings の**閉じた category**と母集団（設計 pipeline.md §6 / §17・`s2-07l.188`）。
//!
//! 3 値だけの verdict は「見て 0 件だった」と「見ていない」を弁別できない（監査 2026-09-12
//! 塊 21・`.175`）。本 module は数える観点を閉じた表（[`Findings`]）にし、verdict の 2 key
//! （`findings` / `population`）を読む・書くの 1 か所（[`Tally`]）を持つ。
//!
//! **判断は lens の領分である**。器が持つのは型と件数と母集団だけで、「この抽象は要るか」は
//! ここに 1 行も書かない（機械が持つと観点が増えるたびに判定が器の側へ漏れる）。

/// lens が数える findings の観点（**閉じた enum**・宣言順が verdict の字面の順・憲法 C2）。
///
/// 既存の観点 3 つ（契約適合 / 歯の非空虚 / 憲法）と過剰設計の 5 種で閉じる。自由文の名を
/// 許すと同じ欠陥が別名で数えられ、件数が集計できない（却下案・設計 §17）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Findings {
    /// 契約（goal / done / write-set）と diff の食い違い。
    ContractFit,
    /// 歯が空虚（assert が字面の出所を区別しない・測る対象が実装に無い）。
    TeethNonvacuous,
    /// 憲法（生成 file `docs/constitution.md`）の条項に反する面。
    Constitution,
    /// 消せる面（重複・到達しない経路・使われない field）。
    Delete,
    /// 標準 library で足りる自作。
    Stdlib,
    /// 言語・道具の既存機能で足りる作り込み。
    Native,
    /// この便が要らない一般化（将来のための引数・設定・抽象）。
    Yagni,
    /// 同じ意味のまま縮む面（分岐の重複・冗長な中間値）。
    Shrink,
}

impl Findings {
    /// 全 variant（**宣言順**＝`findings` の字面の並び順）。
    pub(super) const ALL: [Self; 8] = [
        Self::ContractFit,
        Self::TeethNonvacuous,
        Self::Constitution,
        Self::Delete,
        Self::Stdlib,
        Self::Native,
        Self::Yagni,
        Self::Shrink,
    ];

    /// verdict と雛形に書く字面（**網羅 match**・表に無い名は読む側が断る）。
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::ContractFit => "contract-fit",
            Self::TeethNonvacuous => "teeth-nonvacuous",
            Self::Constitution => "constitution",
            Self::Delete => "delete",
            Self::Stdlib => "stdlib",
            Self::Native => "native",
            Self::Yagni => "yagni",
            Self::Shrink => "shrink",
        }
    }
}

/// lens が読んだ母集団（file 数と行数）。
///
/// **0 は「見ていない」**であって「穴が無い」ではない（C10）。0 を持つ [`Population`] は作らない
/// ——読む側で倒すのではなく、型として存在しない側に置く。
#[derive(Debug)]
pub(super) struct Population {
    /// 読んだ file 数。
    files: u64,
    /// 読んだ行数。
    lines: u64,
}

impl Population {
    /// `files:<n>,lines:<n>` を読む。どちらかが欠け / 数でない / 0 の周は理由 1 行の `Err`
    /// （呼び手が INCONCLUSIVE へ倒す・C11.2）。
    fn parse(text: &str) -> Result<Self, String> {
        let files = number_of(text, "files")?;
        let lines = number_of(text, "lines")?;
        if files == 0 || lines == 0 {
            return Err(format!("population が 0（files:{files},lines:{lines}）＝lens は読んでいない"));
        }
        Ok(Self { files, lines })
    }
}

/// verdict の findings の集計（8 category の件数 + 母集団）。**件数は 0 も持つ**。
#[derive(Debug)]
pub(super) struct Tally {
    /// [`Findings::ALL`] と同じ並びの件数。
    counts: Vec<u64>,
    /// lens が読んだ母集団。
    population: Population,
}

impl Tally {
    /// verdict の 2 key を読む。
    ///
    /// **8 category を全部**（0 も）要る＝欠け・重複・表に無い名・母集団の不備は `Err` で、
    /// 呼び手（`super::lens::parse_lens`）が INCONCLUSIVE へ倒す（測り直せる側・FR14）。
    pub(super) fn parse(findings: &str, population: &str) -> Result<Self, String> {
        let pairs = findings.split(',').map(count_pair).collect::<Result<Vec<_>, String>>()?;
        if let Some((name, _)) = pairs.iter().find(|(name, _)| !known(name)) {
            return Err(format!("findings の category {name} は表に無い"));
        }
        let mut counts = Vec::with_capacity(Findings::ALL.len());
        for found in Findings::ALL {
            let mut hits = pairs.iter().filter(|(name, _)| name == &found.as_str());
            let (_, count) = hits.next().ok_or_else(|| format!("findings に {} が無い", found.as_str()))?;
            if hits.next().is_some() {
                return Err(format!("findings の {} が 2 度出た", found.as_str()));
            }
            counts.push(*count);
        }
        Ok(Self { counts, population: Population::parse(population)? })
    }

    /// `verdict.json` に書く `findings` の字面（**宣言順・8 category を 0 も含めて全部**）。
    ///
    /// lens が並べ替えて出した周も記帳の順は 1 つである（集計する側が順を持つ・C2）。
    pub(super) fn findings_field(&self) -> String {
        Findings::ALL
            .iter()
            .zip(&self.counts)
            .map(|(found, count)| format!("{}:{count}", found.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// `verdict.json` に書く `population` の字面。
    pub(super) fn population_field(&self) -> String {
        format!("files:{},lines:{}", self.population.files, self.population.lines)
    }
}

/// 表に在る category の名か。
fn known(name: &str) -> bool {
    Findings::ALL.iter().any(|found| found.as_str() == name)
}

/// `findings` の 1 項目（`<category>:<件数>`）を読む。件数は 10 進の非負整数だけ。
fn count_pair(item: &str) -> Result<(&str, u64), String> {
    let split = item.trim().split_once(':');
    let (name, count) = split.ok_or_else(|| format!("findings の項目 {item} が <category>:<件数> でない"))?;
    let parsed = count.parse().map_err(|_| format!("findings の件数 {count} が数でない"))?;
    Ok((name, parsed))
}

/// `population` の 1 つの数（`<名>:<n>`）を引く。
fn number_of(text: &str, name: &str) -> Result<u64, String> {
    let found = text.split(',').find_map(|item| item.trim().strip_prefix(name)?.strip_prefix(':'));
    let value = found.ok_or_else(|| format!("population に {name} が無い"))?;
    value.parse().map_err(|_| format!("population の {name} が数でない（{value}）"))
}

#[cfg(test)]
mod tests {
    use super::{Findings, Tally};

    /// 8 category の字面（宣言順）。
    const NAMES: [&str; 8] = [
        "contract-fit",
        "teeth-nonvacuous",
        "constitution",
        "delete",
        "stdlib",
        "native",
        "yagni",
        "shrink",
    ];

    /// 母集団の在る周の `population` の字面。
    const READ: &str = "files:7,lines:42";

    /// 観点は **8 つで閉じ**、[`Findings::ALL`] は宣言順そのままである（順が動けば verdict の
    /// 字面が動く＝集計の並びは表 1 つが持つ）。
    #[test]
    fn pipe_gate_findings_all_lists_the_eight_categories_in_declaration_order() {
        let shown: Vec<&str> = Findings::ALL.iter().map(|found| found.as_str()).collect();
        assert_eq!(shown, NAMES, "8 variant の宣言順");
    }

    /// 読んだ 2 key は**宣言順の字面へ正規化**される（lens が並べ替えても記帳の順は 1 つ・0 も書く）。
    #[test]
    fn pipe_gate_findings_tally_renders_every_category_in_declaration_order() {
        let shuffled = "shrink:5,constitution:2,contract-fit:1,yagni:4,stdlib:3,delete:0,native:0,teeth-nonvacuous:0";
        let tally = Tally::parse(shuffled, READ).expect("8 category と母集団が揃えば読める");
        assert_eq!(
            tally.findings_field(),
            "contract-fit:1,teeth-nonvacuous:0,constitution:2,delete:0,stdlib:3,native:0,yagni:4,shrink:5",
            "宣言順で 8 つとも（0 件も 0 と）書く"
        );
        assert_eq!(tally.population_field(), READ, "母集団はそのまま載る");
    }

    /// 欠け・重複・表に無い名・母集団の不備は**読めない**（呼び手が INCONCLUSIVE へ倒す・C10）。
    #[test]
    fn pipe_gate_findings_tally_refuses_incomplete_categories_and_zero_population() {
        let all = "contract-fit:0,teeth-nonvacuous:0,constitution:0,delete:0,stdlib:0,native:0,yagni:0,shrink:0";
        let short = "contract-fit:0,teeth-nonvacuous:0,constitution:0,delete:0,stdlib:0,native:0,yagni:0";
        let unknown = format!("{all},typo:1");
        let not_a_number = all.replace("shrink:0", "shrink:x");
        let cases = [
            (short, READ, "shrink"),
            ("contract-fit:0,contract-fit:0", READ, "2 度"),
            (unknown.as_str(), READ, "表に無い"),
            ("contract-fit", READ, "<category>:<件数> でない"),
            (not_a_number.as_str(), READ, "数でない"),
            (all, "files:0,lines:42", "population が 0"),
            (all, "files:7,lines:0", "population が 0"),
            (all, "lines:42", "population に files が無い"),
            (all, "files:7", "population に lines が無い"),
        ];
        for (findings, population, reason) in cases {
            let read = Tally::parse(findings, population);
            let err = read.expect_err(&format!("読めてはならない: {findings} / {population}"));
            assert!(err.contains(reason), "理由に {reason} が要る: {err}");
        }
        assert!(Tally::parse(all, READ).is_ok(), "対: 8 つ揃い母集団が 0 でなければ読める");
    }
}
