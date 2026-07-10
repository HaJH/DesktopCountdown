# 백로그

렌더러(계획 1) 완성 후 미룬 개선 항목. 병합을 막지 않는다고 판단해 defer한 것들.
출처는 각 태스크 리뷰와 최종 전체 브랜치 리뷰.

## 코드 개선 (defer)

- **빈 패널 고착 (C3, Minor).** `tick()`의 draw 실패 복구에서 `Compositor::new`는 성공했는데
  `rebuild_panels()`가 일시 실패하면 `panels`가 빈 채로 남고, WorkerW 상실/디스플레이 변경이
  없으면 화면이 빈 채 고착될 수 있다. 단순 재시도는 "모든 모니터 disabled" 정상 케이스와
  구분이 필요하다. → 실패/정상을 구분하는 상태 플래그 필요.
  위치: `src/app.rs` tick 드로우 실패 경로.

- **폰트 조회 캐싱 (Task 9, Minor).** `render/text.rs::family_exists`가 `compose()`마다
  `GetSystemFontCollection` + `FindFamilyName`을 호출한다(measure + paint = 초당 2회).
  DirectWrite 내부 캐시라 저렴하지만, `resolve_family` 결과를 style 키로 캐시하면 제거 가능.

- **reload의 cfg/target 순서 (Task 14, Minor).** `reload()`가 `self.cfg = new_cfg`를 먼저
  대입한 뒤 `to_zoned`를 시도해서, `to_zoned` 실패 시 `self.cfg.target`과 `self.target`이
  어긋난다. 현재 `self.cfg.target`을 읽는 곳은 `to_zoned`뿐이라 무해하지만, 트레이 툴팁이나
  재저장 기능을 추가하면 함정. → zoned 변환 성공 후에만 `self.cfg.target` 반영.

- **초기 Err 로깅 (Task 7, Minor).** `main`이 `logging::init` 이후의 초기 Err(단일 인스턴스
  충돌, config_path 실패)를 `tracing`으로 남기지 않고 그대로 반환한다. 릴리스 빌드는 콘솔이
  없어 보이지 않는다. → `main`의 fallible 본문을 감싸 Err를 로깅 후 반환.

- **Drop의 DestroyWindow 실패 로깅 (Task 11, Minor).** `ChildWindow::Drop`이 `DestroyWindow`
  실패를 완전히 무시한다. Drop에서 panic 금지라 무시 자체는 맞지만 `tracing::debug!` 한 줄이
  진단에 도움.

- **고전 WorkerW 전략 하드닝 (Task 11, Minor).** classic 전략(`SHELLDLL_DefView`를 직계 자식으로
  가진 최상위 창)이 이론상 열린 탐색기 폴더 창(`CabinetWClass`)을 오탐할 여지. 이 머신은
  Progman-child 폴백 경로라 무해. → `enum_cb`에서 매칭된 top-level 클래스가 Progman/WorkerW인지
  확인하는 방어 추가.

- **single_instance 테스트 취약점.** `single_instance::tests::second_acquire_fails_while_the_first_is_held`가
  실제 뮤텍스 이름(`Local\DesktopCountdown`)을 쓰므로, `desktop-countdown.exe`가 실행 중이면
  첫 `acquire()`가 실패해 테스트가 깨진다. 앱을 켜둔 채 `cargo test`를 돌리면 재현. Task 7 리뷰가
  예견함. → 테스트는 프로덕션 뮤텍스 이름을 검증해야 의미가 있으므로, 테스트 실행 전 앱 종료를
  전제로 하거나, "이미 점유된 경우 skip" 처리를 고려.

## 실기 검증 필요 (스크립트로 재현 불가)

개발이 원격 세션(모니터 1개)에서 이뤄져 확인하지 못한 것들. 실제 4-모니터 환경에서 검증 필요.

- Explorer 재시작 후 WorkerW 재획득 + 패널 재구성 (백오프/GIVE_UP_POLL 경로).
- GPU 디바이스 소실(TDR)/절전 복귀 시 컴포지터 재생성으로 화면 복구.
- 60초 백오프 예산 소진과 이후 30초 슬로우폴 복귀.
- 세로/음수 좌표 모니터에서의 앵커·오프셋 실제 배치.
- 트레이 우클릭 메뉴 3항목(한글) 표시·클릭 동작, 종료 시 자식 창 잔류 없음.
- Wallpaper Engine 등 타 벽지 앱과 공존 시 `raise_if_covered` 동작.
- C2(wndproc 재진입 가드) 수정 후 모니터 배율 변경/도킹 반복 시 안정성.

## Won't-fix (기록만)

- `breakdown`의 `.expect()` 메시지가 "jiff 버그"라 단정 — 실 날짜 범위에서 도달 불가.
- `pass()` 8인자 (`#[allow(clippy::too_many_arguments)]`) — 순수 미관.
- 콜드 스타트 시 초 한 번 스킵 — 표시값은 항상 정확, 미관.
