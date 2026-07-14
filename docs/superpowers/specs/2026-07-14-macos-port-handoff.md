# macOS 포팅 핸드오프

macOS 실기 세션에서 이 작업을 이어받을 때 쓰는 프롬프트다.
설계 원본: [`2026-07-14-macos-port-design.md`](./2026-07-14-macos-port-design.md)

---

## 프롬프트 (맥에서 Claude Code에 붙여넣기)

```
DesktopCountdown(Rust, 현재 Windows 전용)에 macOS 지원을 추가한다.
설계는 이미 끝났고 승인됐다: docs/superpowers/specs/2026-07-14-macos-port-design.md
먼저 이 문서를 처음부터 끝까지 읽어라. 설계 결정을 다시 논의하지 말고 그대로 따른다.

핵심 결정 (문서 §1):
- 같은 리포에서 src/platform/{windows,macos} 로 백엔드 분리. Windows의 D2D/DirectWrite
  코드는 이동만 하고 내용은 손대지 않는다 (회귀 위험 0이 요구사항).
- macOS 백엔드는 AppKit(NSWindow 데스크톱 레벨) + CoreText/CoreGraphics 네이티브.
  objc2 계열 크레이트로 통일 (레거시 cocoa/core-graphics/core-text 금지).
- 기능 전체 패리티: 렌더링, 멀티모니터+override, 트레이, 설정 창, 라이브 리로드,
  자동 시작, 단일 인스턴스, 폰트 피커.
- 배포는 미서명 + ad-hoc 서명(필수), universal 바이너리, ditto zip.

지금 할 일 — 설계 문서 §9의 1단계(스파이크)부터 시작한다:
NSWindow 하나에 빨간 사각형만 그리는 최소 실행 파일을 만들고, 실기에서 눈으로 확인한다.
  (a) 데스크톱 아이콘 '아래'에 깔리는가
  (b) 클릭이 통과하는가
  (c) Space를 전환해도 살아있는가
  (d) 화면 상단에 1~2px 틈이 생기는가 (생기면 constrainFrameRect 오버라이드 필요)
  (e) Mission Control / Dock / Cmd-Tab에 안 나오는가
  (f) Stage Manager 켠 상태에서는 어떤가
문서 §5.1에 창 설정값과 레벨 상수가, §8에 미확인 항목이 정리돼 있다.

스파이크 결과를 docs/superpowers/plans/ 아래에 기록하고(이 리포에 spike-result.md 선례가
있다), 결과가 설계 가정과 다르면 진행 전에 알려달라. 가정이 맞으면 §9의 2단계
(구조 리팩터 — Windows 빌드/테스트가 그대로 통과하는지가 이 단계의 합격 조건)로 넘어간다.

빌드/테스트는 맥에서 직접 돌린다. Windows 쪽 코드는 이 머신에서 컴파일할 수 없으니
cfg 분기를 건드릴 때 주의하고, Windows 빌드 검증은 CI(.github/workflows/ci.yml에
macos-15 매트릭스를 추가한다)에 맡긴다.

작업 규칙은 리포의 CLAUDE.md와 사용자 전역 CLAUDE.md를 따른다 (문서/답변은 한국어,
코드와 주석은 영어).
```

---

## 맥에서 미리 준비해둘 것

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Xcode Command Line Tools (CoreText/CoreGraphics 헤더, codesign, lipo, iconutil, sips)
xcode-select --install

git clone https://github.com/HaJH/DesktopCountdown && cd DesktopCountdown
```

macOS 백엔드가 아직 없으므로 클론 직후 `cargo build`는 **실패한다** (정상).
스파이크는 별도의 작은 바이너리(`examples/` 또는 `src/bin/`)로 시작하는 게 빠르다.

## 이 리포의 컨텍스트

- 설계/스펙: `docs/superpowers/specs/`, 구현 계획: `docs/superpowers/plans/`
- 기존 스펙 3개(렌더러·설정 창·줄 목록)를 읽으면 왜 지금 코드가 이 모양인지 알 수 있다.
  특히 `render/mod.rs`의 `ink_span` — CJK 폰트의 line box가 아니라 실제 잉크를 재는 이유가
  거기 적혀 있고, macOS에서도 같은 문제를 같은 방식으로 푼다(`CGPathGetPathBoundingBox`).
- `docs/BACKLOG.md`에 미착수 아이디어가 있다.
