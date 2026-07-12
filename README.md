# DesktopCountdown

바탕화면 배경 레이어에 마감까지 남은 시간을 표시하는 Windows 앱.

```
3m 2w 0d
2544:18:07
```

아랫줄이 남은 총 시간, 윗줄은 개월/주/일 요약입니다(기본 구성 — 표시할 줄과 형식은 설정에서
자유롭게 바꿀 수 있습니다). 데스크톱 아이콘 아래, Wallpaper Engine 같은 다른 배경 앱 위에
그려지므로 다른 창을 가리지 않고 배경도 가리지 않습니다.

## 설치

```
cargo build --release
```

`target\release\desktop-countdown.exe`를 실행하면 콘솔 창 없이 트레이 아이콘만 나타납니다.

## 동작 방식

데스크톱 아이콘 아래에서 벽지를 그리는 `WorkerW` 창을 찾아 그 자식 창을 만들고, DirectComposition으로 픽셀별
알파를 가진 비주얼을 그 위에 얹어 그립니다. `UpdateLayeredWindow`는 자식 창에서 `Ok`를 반환하면서도
아무것도 그리지 않는다는 것을 스파이크에서 확인했기 때문에 DirectComposition을 씁니다(자세한 내용은
`docs/superpowers/plans/spike-result.md`). 이 창은 데스크톱 아이콘 아래, 다른 배경 앱(Wallpaper
Engine 등) 위에 위치하며, 바탕화면 새로 고침에도 살아남습니다.

## 설정

트레이 아이콘 우클릭 → "설정 열기"로 GUI 설정 창을 엽니다(창 UI는 영어). 마감 시각(`target`),
글꼴·크기·색상 등 스타일, 표시할 줄 목록, 배치(anchor/offset), 모니터별 오버라이드를 창에서
편집하며, 변경하면 자동으로 `config.toml`에 저장되고 바탕화면에 약 0.2초 안에 반영됩니다.
슬라이더를 드래그하는 동안에도 초당 10회까지 저장되므로 바탕화면이 드래그를 따라 실시간으로
바뀝니다. 설정 창의 미리보기는 근사치이며, 실제 표시는 바탕화면에서 확인해야 합니다.

글꼴 목록은 검색할 수 있고 각 글꼴 이름을 그 글꼴 모양으로 보여주므로, 한글·중국어·일본어 이름의
글꼴도 이름이 깨지지 않고 미리 그 모양을 확인하며 고를 수 있습니다.

### 줄 목록과 토큰

화면은 위에서 아래로 쌓이는 줄들로 구성됩니다. 각 줄은 토큰이 들어갈 수 있는 템플릿 문자열이고,
줄마다 크기 비율(`[style].size_px`에 곱해집니다)·정렬·색을 정할 수 있습니다. 색을 지정하지 않으면
`[style].color`를 따릅니다. 나머지 스타일(글꼴·굵기·외곽선·그림자·자간·불투명도)은 모든 줄이
공유합니다.

```toml
[[line]]
text = "수능까지"
size_ratio = 0.22
align = "left"
color = "#FFD700"

[[line]]
text = "{daysTotal} days left"
size_ratio = 0.28

[[line]]
text = "{hh}:{mm}:{ss}"
size_ratio = 1.0
```

쓸 수 있는 토큰:

| 토큰 | 의미 |
|---|---|
| `{months}` `{weeks}` `{days}` | 달력 기준 개월 / 개월 뒤 주 / 주 뒤 일 |
| `{daysTotal}` `{hoursTotal}` `{minutesTotal}` `{secondsTotal}` | 남은 총 일·시·분·초 |
| `{hours}` `{minutes}` `{seconds}` | 하루 안의 시(0–23), 시간 안의 분, 분 안의 초 |
| `{hh}` `{mm}` `{ss}` | 2자리 제로패딩 (`{hh}`는 총 시간이라 자릿수를 넘으면 그대로) |

정의되지 않은 토큰은 그대로 화면에 나옵니다(오타를 바로 알 수 있게).

설정 창의 Preset 드롭다운으로 자주 쓰는 구성(Classic, Clock only, D-Day, Days left,
Caption + Clock)을 한 번에 넣을 수 있습니다. 기본값은 Classic —
`{months}m {weeks}w {days}d` 아래에 `{hh}:{mm}:{ss}`입니다.

`%APPDATA%\DesktopCountdown\config.toml`을 직접 편집해도 됩니다. 저장하면 곧바로(약 100ms)
화면에 반영됩니다.

주요 항목은 `target`(마감 시각), `[style]`의 `font_family`·`size_px`·`color`·`mode`,
`[[line]]`(표시할 줄들), `[layout]`의 `anchor`·`offset_px`입니다. 모니터마다 다르게 하려면 설정 창에서 해당 모니터의
오버라이드를 켜거나, `config.toml`에 `[[display]]` 블록을 직접 추가합니다. 전체 스키마는 설계
문서 §4를 보세요.

설정이 잘못되면 이전 설정을 유지하고 트레이 툴팁에 경고를 띄웁니다.
이유는 `%LOCALAPPDATA%\DesktopCountdown\log.txt`에 남습니다.

## 알려진 제약

- 바탕화면 배경 레이어에 그리므로 마우스로 만질 수 없습니다. 모든 조작은 트레이 아이콘과 설정
  파일로 합니다.
- 배경 레이어에 붙는 방법(`WorkerW` 탐색, 필요 시 `Progman`에 `0x052C` 전송)은 문서화되지 않은
  Windows 동작입니다. Explorer가 재시작되면 자동으로 다시 붙지만, 향후 Windows 업데이트로 창
  구조가 바뀌면 깨질 수 있습니다.
- 목표 시각에 도달하면 `00:00:00`에서 멈춥니다. 경과 시간을 세지 않고, 알림도 띄우지 않습니다.

## 문서

- 설계: `docs/superpowers/specs/2026-07-10-desktop-countdown-design.md`
- 스파이크 결과: `docs/superpowers/plans/spike-result.md`
- 구현 계획: `docs/superpowers/plans/`
