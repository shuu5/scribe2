# scribe2

## Beads Issue Tracker (bd) + scribe

タスク追跡は **bd (beads)**。SessionStart hook が `bd prime` で bd 基礎の文脈を毎セッション注入する（**運用ルールと詳細の SSOT = `.beads/PRIME.md`**）。本節は PRIME が注入されない bd 未導入時だけの最小フォールバック。

- **タスク = beads / 知識 = repo と host の既存 carrier**: 永続・横断の作業は bd issue で追跡。知見は repo 内の教訓 doc と auto-memory に置き、**`bd remember/recall/memories` は使わない**（理由・詳細は PRIME）。
- **役割を帯びた規約（誰が create/dep/close/dolt push するか・終了プロトコル）の SSOT は scribe plugin の role 別 SessionStart 注入**（admin / worker / consult）。PRIME は role 中立な基礎のみを持つ。
