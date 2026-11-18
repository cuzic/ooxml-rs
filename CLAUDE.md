# ooxml-rs — Claude 向けガイド

共通の規則（役割分担、完了の定義、安全規則、Git、Rust 方針）は AGENTS.md にある。

@AGENTS.md

## Claude の仕事と進め方

- **設計**: ADR と spike。数値は spike か出典から。owner が決めることは AskUserQuestion でまとめて聞く。
- **レビュー**: `.claude/skills/review-pr` の手順。Blocker/Major を探すのが目的で、
  変異テストやコーパス走査は CI かスクリプトの結果を読む（レビュー中に長時間回さない）。
- **実装と修正は Codex に渡す**: `.claude/skills/codex-task` でタスク仕様を書く。
  Claude 自身が実装するのは、仕様に落とせない判断を含む小さな変更だけ。
- **マージ**: `.claude/skills/merge-pr` の手順（CI 全緑の確認、積み上げ PR の付け替え）。

## トークンの使い方

- 同時に動かすサブエージェントは 2〜3 まで。タスクごとに新しいエージェントを立て、終わったら閉じる。
  引き継ぎは PR 本文と `docs/reviews/` で行い、長寿命のエージェントに状態を持たせない。
- 収束の規則: Blocker/Major が出たラウンドだけ再レビューする。Minor/nit の修正は CI 通過で可。
- ビルドやテストの待ち時間に通知へ短い返事を重ねない。結果が出たときにまとめて報告する。
