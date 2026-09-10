/* ============================================================
 * OpenLess 远程输入 — 手机端录音页
 * 纯静态,无外部依赖。通过 WSS 把 16kHz/单声道/16bit LE PCM
 * 实时推送给 PC 端 Rust 服务。
 *
 * 显示语言跟随 PC 端界面语言：Rust 在返回首页时把 window.__OL_LANG__
 * 注入成 PC 当前 locale（前端切换语言时经 set_remote_locale 命令同步）。
 * ========================================================== */
(function () {
  'use strict';

  // ============================================================
  // i18n —— 文案字典（与 PC 端 src/i18n 对齐的 5 种语言）
  // ============================================================
  var I18N = {
    'zh-CN': {
      wakeLockLabel: "录音时保持亮屏",
      wakeLockHint: "息屏会结束本段录音，电脑继续处理已收到的部分。",
      wakeLockActive: "屏幕保持亮起，录音结束后允许自动息屏。",
      wakeLockUnavailable: "浏览器或系统未允许保持亮屏；息屏后电脑会处理已收到的录音。",
      interrupted: "录音已中断，电脑继续识别已收到的部分…",
      offlineRecording: "连接已断开，电脑会继续处理已收到的录音。重连后可查看结果。",
      recovering: "电脑正在处理上次录音…",
      recovered: "已找回上次识别结果",
      recoveryRetry: "识别未完成，录音已保存在电脑历史记录中，可重新转录。",
      recoveryUnavailable: "暂未找到结果，请到电脑的历史记录中查看。",
      title: 'OpenLess 远程输入',
      brandTitle: 'OpenLess 远程输入',
      brandSub: '在手机上录音，实时输入到电脑',
      pinFieldLabel: '配对码（电脑上显示的 6 位数字）',
      btnConnect: '连接',
      btnConnecting: '连接中…',
      modeToggle: '点按',
      insertLabel: '电脑落字',
      modeHold: '按住',
      offlineTitle: '连接已断开',
      offlineSub: '与电脑的连接已中断。',
      btnReconnect: '重新连接',
      certTip: "如遇证书提示，请核对地址与电脑显示一致，再按“首次设置：信任此电脑”完成安装和完全信任。",
      tipToggle: '点击大按钮开始录音，再次点击结束并识别。',
      tipHold: '按住大按钮说话，松开结束并识别。',
      labelToggleIdle: '点击开始',
      labelToggleRec: '点击结束',
      labelHoldIdle: '按住说话',
      labelHoldRec: '松开结束',
      ready: '准备就绪',
      preparingMic: '正在准备麦克风…',
      preparingBackend: '后端准备中…',
      statusRecording: '🎤 录音中',
      statusTranscribing: '🔄 识别中',
      statusPolishing: '✨ 润色中',
      statusDone: '✅ 已输入 {n} 字',
      cancelled: '已取消',
      connLost: '连接已断开',
      errPinFormat: '请输入 6 位数字配对码。',
      errPinWrong: '配对码错误，请重试。',
      errPinLocked: '配对已锁定，请在电脑上重新生成配对码。',
      errConnFail: '连接失败。多半是手机未信任电脑证书，请先信任证书后重试。',
      errConnCreate: '无法建立连接，请检查网络。',
      errConnTimeout: '连接超时。多半是手机未信任电脑证书，请按下方说明信任后重试。',
      busy: '电脑忙：{reason}',
      busyDefault: '请稍候',
      micDenied: '❌ 麦克风权限被拒绝，请在浏览器设置中允许。',
      micNotFound: '❌ 未找到可用麦克风。',
      micBusy: '❌ 麦克风被其他应用占用。',
      micTimeout: '❌ 麦克风准备超时，请重试。',
      pcmQueueOverflow: '❌ 音频缓存已满，请重试。',
      micUnknown: '❌ 无法启动录音{name}。',
      errGeneric: '发生错误',
      helpTitle: "首次设置：信任此电脑",
      helpAndroid: "安卓：下载 CA 证书，在系统设置的“安装证书 → CA 证书”中安装，再回到本页。菜单名称因设备而异。",
      helpIos: "iPhone / iPad：下载描述文件，打开“设置 → 通用 → VPN 与设备管理”安装；再到“通用 → 关于本机 → 证书信任设置”，为 OpenLess Remote Input CA 打开“完全信任”，返回 Safari 刷新。每台电脑只需设置一次；旧版证书需要重新安装。",
      helpDownloadCert: "↓ iPhone：下载描述文件",
      helpDownloadAndroid: "↓ 安卓：下载 CA 证书",
      helpTrustWarning: "首次证书下载无法验证电脑身份，恶意局域网设备可能通过中间人攻击替换根证书。仅在可信的家庭或私人网络中安装，勿在公共或共享网络操作。根证书具备签发能力，私钥保存在这台电脑；不再使用时请从手机移除。",
      helpCopyLink: '⧉ 复制链接',
      helpCopied: '已复制 ✓',
      copy: '复制',
      copied: '已复制 ✓',
    },
    'zh-TW': {
      wakeLockLabel: "錄音時保持螢幕開啟",
      wakeLockHint: "螢幕關閉會結束本段錄音，電腦繼續處理已收到的部分。",
      wakeLockActive: "螢幕保持開啟，錄音結束後允許自動關閉螢幕。",
      wakeLockUnavailable: "瀏覽器或系統未允許保持螢幕開啟；電腦會處理已收到的錄音。",
      interrupted: "錄音已中斷，電腦繼續辨識已收到的部分…",
      offlineRecording: "連線已中斷，電腦會繼續處理已收到的錄音。重新連線後可查看結果。",
      recovering: "電腦正在處理上次錄音…",
      recovered: "已找回上次辨識結果",
      recoveryRetry: "辨識未完成，錄音已保存在電腦歷史記錄中，可重新轉錄。",
      recoveryUnavailable: "暫未找到結果，請到電腦的歷史記錄中查看。",
      title: 'OpenLess 遠端輸入',
      brandTitle: 'OpenLess 遠端輸入',
      brandSub: '在手機上錄音，即時輸入到電腦',
      pinFieldLabel: '配對碼（電腦上顯示的 6 位數字）',
      btnConnect: '連線',
      btnConnecting: '連線中…',
      modeToggle: '點按',
      insertLabel: '電腦落字',
      modeHold: '按住',
      offlineTitle: '連線已中斷',
      offlineSub: '與電腦的連線已中斷。',
      btnReconnect: '重新連線',
      certTip: "如遇憑證提示，請核對位址與電腦顯示一致，再依「首次設定：信任這台電腦」完成安裝與完全信任。",
      tipToggle: '點擊大按鈕開始錄音，再次點擊結束並辨識。',
      tipHold: '按住大按鈕說話，放開結束並辨識。',
      labelToggleIdle: '點擊開始',
      labelToggleRec: '點擊結束',
      labelHoldIdle: '按住說話',
      labelHoldRec: '放開結束',
      ready: '準備就緒',
      preparingMic: '正在準備麥克風…',
      preparingBackend: '後端準備中…',
      statusRecording: '🎤 錄音中',
      statusTranscribing: '🔄 辨識中',
      statusPolishing: '✨ 潤飾中',
      statusDone: '✅ 已輸入 {n} 字',
      cancelled: '已取消',
      connLost: '連線已中斷',
      errPinFormat: '請輸入 6 位數字配對碼。',
      errPinWrong: '配對碼錯誤，請重試。',
      errPinLocked: '配對已鎖定，請在電腦上重新產生配對碼。',
      errConnFail: '連線失敗。多半是手機未信任電腦憑證，請先信任憑證後重試。',
      errConnCreate: '無法建立連線，請檢查網路。',
      errConnTimeout: '連線逾時。多半是手機未信任電腦憑證，請依下方說明信任後重試。',
      busy: '電腦忙碌：{reason}',
      busyDefault: '請稍候',
      micDenied: '❌ 麥克風權限遭拒，請在瀏覽器設定中允許。',
      micNotFound: '❌ 找不到可用的麥克風。',
      micBusy: '❌ 麥克風被其他應用程式佔用。',
      micTimeout: '❌ 麥克風準備逾時，請重試。',
      pcmQueueOverflow: '❌ 音訊暫存已滿，請重試。',
      micUnknown: '❌ 無法啟動錄音{name}。',
      errGeneric: '發生錯誤',
      helpTitle: "首次設定：信任這台電腦",
      helpAndroid: "Android：下載 CA 憑證，在系統設定的「安裝憑證 → CA 憑證」中安裝，再返回本頁。選單名稱依裝置而異。",
      helpIos: "iPhone / iPad：下載描述檔，開啟「設定 → 一般 → VPN 與裝置管理」安裝；再到「一般 → 關於本機 → 憑證信任設定」，為 OpenLess Remote Input CA 開啟「完全信任」，返回 Safari 重新整理。每台電腦只需設定一次；舊版憑證需要重新安裝。",
      helpDownloadCert: "↓ iPhone：下載描述檔",
      helpDownloadAndroid: "↓ Android：下載 CA 憑證",
      helpTrustWarning: "首次憑證下載無法驗證電腦身分，惡意區域網路裝置可能透過中間人攻擊替換根憑證。僅在可信任的家庭或私人網路中安裝，請勿在公共或共享網路操作。根憑證能簽發憑證，私密金鑰保存在這台電腦；不再使用時請從手機移除。",
      helpCopyLink: '⧉ 複製連結',
      helpCopied: '已複製 ✓',
      copy: '複製',
      copied: '已複製 ✓',
    },
    en: {
      wakeLockLabel: "Keep screen awake while recording",
      wakeLockHint: "Screen lock ends this recording. The computer processes the audio already received.",
      wakeLockActive: "Screen stays awake until recording ends.",
      wakeLockUnavailable: "The browser or system did not allow screen wake lock. Received audio will still be processed.",
      interrupted: "Recording interrupted. The computer is processing the audio received…",
      offlineRecording: "Disconnected. The computer continues processing received audio. Reconnect to see the result.",
      recovering: "The computer is processing your last recording…",
      recovered: "Last transcription recovered",
      recoveryRetry: "Transcription failed. The recording is saved in computer history and can be retried.",
      recoveryUnavailable: "Result unavailable. Please check history on the computer.",
      title: 'OpenLess Remote Input',
      brandTitle: 'OpenLess Remote Input',
      brandSub: 'Record on your phone, type to your computer in real time',
      pinFieldLabel: 'Pairing code (6 digits shown on your computer)',
      btnConnect: 'Connect',
      btnConnecting: 'Connecting…',
      modeToggle: 'Tap',
      insertLabel: 'Type on PC',
      modeHold: 'Hold',
      offlineTitle: 'Disconnected',
      offlineSub: 'The connection to your computer was lost.',
      btnReconnect: 'Reconnect',
      certTip: "If a certificate warning appears, check that the address matches your computer, then follow “First-time setup: trust this computer” to install and fully trust the certificate.",
      tipToggle: 'Tap the big button to start recording, tap again to finish and transcribe.',
      tipHold: 'Hold the big button to talk, release to finish and transcribe.',
      labelToggleIdle: 'Tap to start',
      labelToggleRec: 'Tap to stop',
      labelHoldIdle: 'Hold to talk',
      labelHoldRec: 'Release to stop',
      ready: 'Ready',
      preparingMic: 'Preparing microphone…',
      preparingBackend: 'Preparing backend…',
      statusRecording: '🎤 Recording',
      statusTranscribing: '🔄 Transcribing',
      statusPolishing: '✨ Polishing',
      statusDone: '✅ Inserted {n} chars',
      cancelled: 'Cancelled',
      connLost: 'Connection lost',
      errPinFormat: 'Please enter the 6-digit pairing code.',
      errPinWrong: 'Wrong pairing code, please try again.',
      errPinLocked: 'Pairing locked. Please regenerate the code on your computer.',
      errConnFail: 'Connection failed — usually the phone does not trust the computer certificate. Trust it, then retry.',
      errConnCreate: 'Could not connect. Please check your network.',
      errConnTimeout: 'Connection timed out — the phone likely does not trust the certificate. Follow the steps below to trust it, then retry.',
      busy: 'Computer busy: {reason}',
      busyDefault: 'please wait',
      micDenied: '❌ Microphone permission denied. Please allow it in browser settings.',
      micNotFound: '❌ No microphone available.',
      micBusy: '❌ Microphone is in use by another app.',
      micTimeout: '❌ Microphone setup timed out. Please try again.',
      pcmQueueOverflow: '❌ Audio buffer is full. Please try again.',
      micUnknown: '❌ Could not start recording{name}.',
      errGeneric: 'An error occurred',
      helpTitle: "First-time setup: trust this computer",
      helpAndroid: "Android: download the CA certificate, install it under system Settings → Install a certificate → CA certificate, then return here. Menu names vary by device.",
      helpIos: "iPhone / iPad: download the profile and install it in Settings → General → VPN & Device Management. Then enable full trust for OpenLess Remote Input CA in General → About → Certificate Trust Settings and reload Safari. Set up once per computer; certificates from older versions need this one-time replacement.",
      helpDownloadCert: "↓ iPhone: download profile",
      helpDownloadAndroid: "↓ Android: download CA",
      helpTrustWarning: "The initial certificate download cannot verify the computer's identity; a malicious device on the LAN could replace the root certificate in a man-in-the-middle attack. Install it only on a trusted home or private network, never on a public or shared network. The root CA can issue certificates and its private key stays on this computer; remove it from your phone when no longer needed.",
      helpCopyLink: '⧉ Copy link',
      helpCopied: 'Copied ✓',
      copy: 'Copy',
      copied: 'Copied ✓',
    },
    ja: {
      wakeLockLabel: "録音中は画面をオンにする",
      wakeLockHint: "画面をロックすると録音を終了し、受信済みの音声をパソコンで処理します。",
      wakeLockActive: "録音が終わるまで画面をオンに保ちます。",
      wakeLockUnavailable: "ブラウザーまたはシステムが画面の維持を許可しませんでした。受信済みの音声は処理されます。",
      interrupted: "録音が中断されました。受信済みの音声をパソコンで処理しています…",
      offlineRecording: "接続が切れました。受信済みの音声の処理は続きます。再接続すると結果を確認できます。",
      recovering: "前回の録音をパソコンで処理しています…",
      recovered: "前回の文字起こし結果を復元しました",
      recoveryRetry: "文字起こしが完了しませんでした。録音はパソコンの履歴に保存され、再試行できます。",
      recoveryUnavailable: "結果が見つかりません。パソコンの履歴を確認してください。",
      title: 'OpenLess リモート入力',
      brandTitle: 'OpenLess リモート入力',
      brandSub: 'スマホで録音し、リアルタイムでパソコンに入力',
      pinFieldLabel: 'ペアリングコード（パソコンに表示される6桁の数字）',
      btnConnect: '接続',
      btnConnecting: '接続中…',
      modeToggle: 'タップ',
      insertLabel: 'PCに入力',
      modeHold: '長押し',
      offlineTitle: '接続が切断されました',
      offlineSub: 'パソコンとの接続が切断されました。',
      btnReconnect: '再接続',
      certTip: "証明書の警告が表示されたら、アドレスがコンピュータの表示と一致することを確認し、「初回設定：このコンピュータを信頼」の手順でインストールと完全な信頼を行ってください。",
      tipToggle: '大きいボタンをタップして録音開始、もう一度タップで終了して認識します。',
      tipHold: '大きいボタンを長押しして話し、離すと終了して認識します。',
      labelToggleIdle: 'タップで開始',
      labelToggleRec: 'タップで終了',
      labelHoldIdle: '長押しで話す',
      labelHoldRec: '離して終了',
      ready: '準備完了',
      preparingMic: 'マイクを準備中…',
      preparingBackend: 'バックエンドを準備しています…',
      statusRecording: '🎤 録音中',
      statusTranscribing: '🔄 認識中',
      statusPolishing: '✨ 整文中',
      statusDone: '✅ {n}文字を入力しました',
      cancelled: 'キャンセルしました',
      connLost: '接続が切断されました',
      errPinFormat: '6桁の数字のペアリングコードを入力してください。',
      errPinWrong: 'ペアリングコードが違います。もう一度お試しください。',
      errPinLocked: 'ペアリングがロックされました。パソコンでコードを再生成してください。',
      errConnFail: '接続に失敗しました。多くはスマホがパソコンの証明書を信頼していないためです。証明書を信頼してから再試行してください。',
      errConnCreate: '接続できません。ネットワークを確認してください。',
      errConnTimeout: '接続がタイムアウトしました。多くはスマホが証明書を信頼していないためです。下の手順で信頼してから再試行してください。',
      busy: 'パソコンがビジー状態です：{reason}',
      busyDefault: 'お待ちください',
      micDenied: '❌ マイクの許可が拒否されました。ブラウザの設定で許可してください。',
      micNotFound: '❌ 利用可能なマイクが見つかりません。',
      micBusy: '❌ マイクが他のアプリで使用されています。',
      micTimeout: '❌ マイクの準備がタイムアウトしました。もう一度お試しください。',
      pcmQueueOverflow: '❌ 音声バッファがいっぱいです。もう一度お試しください。',
      micUnknown: '❌ 録音を開始できませんでした{name}。',
      errGeneric: 'エラーが発生しました',
      helpTitle: "初回設定：このコンピュータを信頼",
      helpAndroid: "Android：CA 証明書をダウンロードし、システム設定の「証明書のインストール → CA 証明書」でインストールしてから戻ってください。項目名は端末により異なります。",
      helpIos: "iPhone / iPad：プロファイルをダウンロードし、「設定 → 一般 → VPN とデバイス管理」でインストールします。次に「一般 → 情報 → 証明書信頼設定」で OpenLess Remote Input CA を完全に信頼し、Safari を再読み込みしてください。各コンピュータで一度だけ必要です。旧バージョンの証明書は一度入れ替えてください。",
      helpDownloadCert: "↓ iPhone：プロファイルをダウンロード",
      helpDownloadAndroid: "↓ Android：CA をダウンロード",
      helpTrustWarning: "初回の証明書ダウンロードではコンピューターの身元を確認できず、LAN 上の悪意あるデバイスが中間者攻撃でルート証明書を置き換える可能性があります。信頼できる家庭内またはプライベートネットワークでのみインストールし、公共または共有ネットワークでは操作しないでください。ルート CA は証明書を発行でき、秘密鍵はこのコンピューターに保存されます。不要になったらスマートフォンから削除してください。",
      helpCopyLink: '⧉ リンクをコピー',
      helpCopied: 'コピーしました ✓',
      copy: 'コピー',
      copied: 'コピー済み ✓',
    },
    ko: {
      wakeLockLabel: "녹음 중 화면 켜짐 유지",
      wakeLockHint: "화면을 잠그면 녹음이 끝나고 컴퓨터가 이미 받은 오디오를 처리합니다.",
      wakeLockActive: "녹음이 끝날 때까지 화면을 켜진 상태로 유지합니다.",
      wakeLockUnavailable: "브라우저 또는 시스템이 화면 켜짐 유지를 허용하지 않았습니다. 수신한 오디오는 계속 처리됩니다.",
      interrupted: "녹음이 중단되었습니다. 컴퓨터가 받은 오디오를 처리하고 있습니다…",
      offlineRecording: "연결이 끊겼습니다. 받은 오디오는 계속 처리됩니다. 다시 연결하면 결과를 볼 수 있습니다.",
      recovering: "컴퓨터가 마지막 녹음을 처리하고 있습니다…",
      recovered: "마지막 음성 인식 결과를 복구했습니다",
      recoveryRetry: "음성 인식을 완료하지 못했습니다. 녹음은 컴퓨터 기록에 저장되며 다시 시도할 수 있습니다.",
      recoveryUnavailable: "결과를 찾을 수 없습니다. 컴퓨터의 기록을 확인해 주세요.",
      title: 'OpenLess 원격 입력',
      brandTitle: 'OpenLess 원격 입력',
      brandSub: '휴대폰으로 녹음하여 실시간으로 컴퓨터에 입력',
      pinFieldLabel: '페어링 코드 (컴퓨터에 표시된 6자리 숫자)',
      btnConnect: '연결',
      btnConnecting: '연결 중…',
      modeToggle: '탭',
      insertLabel: 'PC에 입력',
      modeHold: '길게 누르기',
      offlineTitle: '연결이 끊겼습니다',
      offlineSub: '컴퓨터와의 연결이 끊겼습니다.',
      btnReconnect: '다시 연결',
      certTip: "인증서 경고가 나타나면 주소가 컴퓨터에 표시된 주소와 일치하는지 확인한 뒤 “최초 설정: 이 컴퓨터 신뢰”에 따라 인증서를 설치하고 완전히 신뢰하세요.",
      tipToggle: '큰 버튼을 탭하여 녹음을 시작하고, 다시 탭하면 종료 후 인식합니다.',
      tipHold: '큰 버튼을 길게 눌러 말하고, 떼면 종료 후 인식합니다.',
      labelToggleIdle: '탭하여 시작',
      labelToggleRec: '탭하여 종료',
      labelHoldIdle: '눌러서 말하기',
      labelHoldRec: '떼면 종료',
      ready: '준비 완료',
      preparingMic: '마이크 준비 중…',
      preparingBackend: '백엔드 준비 중…',
      statusRecording: '🎤 녹음 중',
      statusTranscribing: '🔄 인식 중',
      statusPolishing: '✨ 다듬는 중',
      statusDone: '✅ {n}자 입력함',
      cancelled: '취소됨',
      connLost: '연결이 끊겼습니다',
      errPinFormat: '6자리 숫자 페어링 코드를 입력하세요.',
      errPinWrong: '페어링 코드가 잘못되었습니다. 다시 시도하세요.',
      errPinLocked: '페어링이 잠겼습니다. 컴퓨터에서 코드를 다시 생성하세요.',
      errConnFail: '연결에 실패했습니다. 대개 휴대폰이 컴퓨터 인증서를 신뢰하지 않기 때문입니다. 인증서를 신뢰한 후 다시 시도하세요.',
      errConnCreate: '연결할 수 없습니다. 네트워크를 확인하세요.',
      errConnTimeout: '연결 시간이 초과되었습니다. 대개 인증서를 신뢰하지 않기 때문입니다. 아래 안내대로 신뢰 후 다시 시도하세요.',
      busy: '컴퓨터가 사용 중입니다: {reason}',
      busyDefault: '잠시 기다려 주세요',
      micDenied: '❌ 마이크 권한이 거부되었습니다. 브라우저 설정에서 허용하세요.',
      micNotFound: '❌ 사용 가능한 마이크가 없습니다.',
      micBusy: '❌ 마이크가 다른 앱에서 사용 중입니다.',
      micTimeout: '❌ 마이크 준비 시간이 초과되었습니다. 다시 시도하세요.',
      pcmQueueOverflow: '❌ 오디오 버퍼가 가득 찼습니다. 다시 시도하세요.',
      micUnknown: '❌ 녹음을 시작할 수 없습니다{name}.',
      errGeneric: '오류가 발생했습니다',
      helpTitle: "최초 설정: 이 컴퓨터 신뢰",
      helpAndroid: "Android: CA 인증서를 다운로드하고 시스템 설정 → 인증서 설치 → CA 인증서에서 설치한 뒤 돌아오세요. 메뉴 이름은 기기마다 다릅니다.",
      helpIos: "iPhone / iPad: 프로파일을 다운로드하고 설정 → 일반 → VPN 및 기기 관리에서 설치하세요. 일반 → 정보 → 인증서 신뢰 설정에서 OpenLess Remote Input CA를 완전히 신뢰한 뒤 Safari를 새로고침하세요. 컴퓨터마다 한 번만 설정하면 됩니다. 이전 버전의 인증서는 한 번 교체해야 합니다.",
      helpDownloadCert: "↓ iPhone: 프로파일 다운로드",
      helpDownloadAndroid: "↓ Android: CA 다운로드",
      helpTrustWarning: "최초 인증서 다운로드에서는 컴퓨터의 신원을 확인할 수 없으며, LAN의 악성 기기가 중간자 공격으로 루트 인증서를 바꿀 수 있습니다. 신뢰할 수 있는 가정용 또는 사설 네트워크에서만 설치하고 공용 또는 공유 네트워크에서는 진행하지 마세요. 루트 CA는 인증서를 발급할 수 있고 개인 키는 이 컴퓨터에 저장됩니다. 더 이상 사용하지 않으면 휴대폰에서 제거하세요.",
      helpCopyLink: '⧉ 링크 복사',
      helpCopied: '복사됨 ✓',
      copy: '복사',
      copied: '복사됨 ✓',
    },
  };

  // 解析显示语言：优先 PC 注入的 window.__OL_LANG__，回退手机系统语言。
  var LANG = (function () {
    var supported = { 'zh-CN': 1, 'zh-TW': 1, en: 1, ja: 1, ko: 1 };
    var injected = (window.__OL_LANG__ || '').trim();
    if (supported[injected]) return injected;
    var nav = (navigator.language || '').toLowerCase();
    if (nav.indexOf('zh') === 0) {
      if (nav.indexOf('hant') >= 0 || nav.indexOf('tw') >= 0 || nav.indexOf('hk') >= 0 || nav.indexOf('mo') >= 0) return 'zh-TW';
      return 'zh-CN';
    }
    if (nav.indexOf('ja') === 0) return 'ja';
    if (nav.indexOf('ko') === 0) return 'ko';
    if (nav.indexOf('en') === 0) return 'en';
    return 'zh-CN';
  })();
  var L = I18N[LANG] || I18N['zh-CN'];

  // 极简插值：把 "{n}" / "{reason}" / "{name}" 替换成对应值。
  function fmt(tpl, vars) {
    return String(tpl).replace(/\{(\w+)\}/g, function (_, k) {
      return (vars && vars[k] != null) ? vars[k] : '';
    });
  }

  // 把 index.html 里带 data-i18n 的静态文案按当前语言渲染。
  function applyStaticI18n() {
    try { document.title = L.title; } catch (e) {}
    var nodes = document.querySelectorAll('[data-i18n]');
    for (var i = 0; i < nodes.length; i++) {
      var key = nodes[i].getAttribute('data-i18n');
      if (L[key] != null) nodes[i].textContent = L[key];
    }
  }

  // ---------- 常量 ----------
  var TARGET_SR = 16000;            // 目标采样率,必须与 PC 端一致
  var MODE_KEY = 'ol_remote_mode';  // localStorage 键:录音方式
  var PIN_KEY = 'ol_remote_pin';    // localStorage 键:上次成功的配对码
  var INSERT_KEY = 'ol_remote_insert'; // localStorage 键:电脑落字开关(默认开)
  var WAKE_LOCK_KEY = 'ol_remote_wake_lock';
  var RECOVERY_KEY = 'ol_remote_recovery_session';
  var MIC_PREP_TIMEOUT_MS = 10000;  // 麦克风准备超时:超过则判失败让用户重试,避免无限卡"准备中"
  var PCM_QUEUE_MAX_BYTES = 128 * 1024;

  // ---------- DOM ----------
  var $ = function (id) { return document.getElementById(id); };
  var screenPin = $('screen-pin');
  var screenRec = $('screen-rec');
  var screenOffline = $('screen-offline');

  var pinInput = $('pin-input');
  var pinError = $('pin-error');
  var btnConnect = $('btn-connect');

  var recordBtn = $('btn-record');
  var recordLabel = $('record-label');
  var statusBar = $('status-bar');
  var statusText = $('status-text');
  var statusIcon = $('status-icon');
  var statusDots = $('status-dots');
  var resultWrap = $('result-wrap');
  var resultText = $('result-text');
  var resultCopy = $('result-copy');
  var levelBar = $('level-bar');
  var recTip = $('rec-tip');
  var modeSwitch = $('mode-switch');
  var insertSwitch = $('insert-switch');
  var wakeLockSwitch = $('wake-lock-switch');
  var wakeLockHint = $('wake-lock-hint');

  var btnReconnect = $('btn-reconnect');
  var offlineReason = $('offline-reason');
  var copyCertBtn = $('copy-cert-link');

  // ---------- 状态 ----------
  var ws = null;
  var authed = false;
  var recording = false;          // 是否正在录音(决定是否 send 音频)
  var startSent = false;          // 本次录音的 {type:'start'} 是否已真正发出(等 ensureAudio 异步就绪后才发)
  var busy = false;               // PC 端忙,本次禁用
  var mode = readMode();          // 'toggle' | 'hold'
  var lastPin = '';
  var remoteSessionId = '';
  var remoteSequence = 0;
  var finishAfterStarted = '';      // ACK 前松手/取消：'stop' | 'cancel' | ''
  var pendingPcm = [];
  var pendingPcmBytes = 0;
  var awaitingResult = false;
  var savedRecovery = readRecoverySession();
  var recoverySessionId = savedRecovery.sessionId;
  var recoveryKey = savedRecovery.key;
  var recoveryTimer = null;
  var wakeLock = null;
  var wakeLockGeneration = 0;
  var wakeLockPending = null;

  // 音频相关
  var audioCtx = null;
  var mediaStream = null;
  var sourceNode = null;
  var workletNode = null;
  var scriptNode = null;
  var workletUrl = null;
  var usingWorklet = false;
  // 音频代际计数:每次重置/释放音频时自增。getUserMedia 可能在 withTimeout 超时后
  // 迟到 resolve,若不校验代际,迟到的 stream 会泄漏活跃麦克风轨道,甚至覆盖丢失
  // 用户重试成功后的新流。
  var audioGen = 0;
  // ScriptProcessor 兜底用的重采样状态(跨块保留)
  var resampleState = { phase: 0, last: 0, hasLast: false };

  // ============================================================
  // 配对码持久化(localStorage)
  // ============================================================
  function readPin() {
    try {
      var p = localStorage.getItem(PIN_KEY);
      return /^\d{6}$/.test(p || '') ? p : '';
    } catch (e) { return ''; }
  }
  function writePin(p) {
    try { if (/^\d{6}$/.test(p)) localStorage.setItem(PIN_KEY, p); } catch (e) {}
  }
  function clearPin() {
    try { localStorage.removeItem(PIN_KEY); } catch (e) {}
    saveRecoverySession('');
  }

  // 恢复凭据仅用于本次随机会话；不请求电脑历史记录列表。
  function readRecoverySession() {
    try {
      var saved = JSON.parse(localStorage.getItem(RECOVERY_KEY) || 'null');
      if (saved && validSessionId(saved.sessionId) && validSessionId(saved.key)) return saved;
    } catch (e) {}
    return { sessionId: '', key: '' };
  }
  function validSessionId(id) {
    return typeof id === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id);
  }
  function saveRecoverySession(id, key) {
    recoverySessionId = validSessionId(id) && validSessionId(key) ? id : '';
    recoveryKey = recoverySessionId ? key : '';
    try {
      if (recoverySessionId) localStorage.setItem(RECOVERY_KEY, JSON.stringify({ sessionId: recoverySessionId, key: recoveryKey }));
      else localStorage.removeItem(RECOVERY_KEY);
    } catch (e) {}
  }
  function clearRecoveryTimer() {
    if (recoveryTimer) { clearTimeout(recoveryTimer); recoveryTimer = null; }
  }
  function requestRecovery() {
    clearRecoveryTimer();
    if (!authed || document.hidden || recording || startSent || !recoverySessionId) return;
    wsSendJSON({ type: 'recover', sessionId: recoverySessionId, recoveryKey: recoveryKey });
    // 唤醒后的旧连接可能仍显示 OPEN，却再也收不到数据；超时重新认证。
    recoveryTimer = setTimeout(function () {
      recoveryTimer = null;
      if (!recording && authed && !document.hidden) {
        var pin = readPin();
        if (pin) connect(pin);
      }
    }, 8000);
  }
  function handleRecovery(msg) {
    if (msg.sessionId !== recoverySessionId || recording || startSent) return;
    clearRecoveryTimer();
    clearWorkTimeout();
    var recovery = msg.recovery || {};
    awaitingResult = recovery.kind === 'pending';
    updateRecordBtnUI();
    if (recovery.kind === 'pending') {
      setStatus(L.recovering, 'work');
      if (!document.hidden) recoveryTimer = setTimeout(requestRecovery, 1500);
    } else if (recovery.kind === 'completed') {
      showResult(recovery.text);
      setStatus(L.recovered, 'ok');
    } else if (recovery.kind === 'failed') {
      setStatus(recovery.hasAudioRecording ? L.recoveryRetry : L.recoveryUnavailable, 'error');
    } else {
      saveRecoverySession('');
      setStatus(L.recoveryUnavailable, 'error');
    }
  }

  function shouldKeepAwake() {
    return recording && !document.hidden && wakeLockSwitch && wakeLockSwitch.checked;
  }
  function releaseWakeLock() {
    wakeLockGeneration++;
    wakeLockPending = null;
    var previous = wakeLock;
    wakeLock = null;
    if (previous) previous.release().catch(function () {});
  }
  function acquireWakeLock() {
    if (!shouldKeepAwake() || wakeLock || wakeLockPending !== null) return;
    if (!navigator.wakeLock || !navigator.wakeLock.request) {
      if (wakeLockHint) wakeLockHint.textContent = L.wakeLockUnavailable;
      return;
    }
    var generation = wakeLockGeneration;
    wakeLockPending = generation;
    navigator.wakeLock.request('screen').then(function (sentinel) {
      if (wakeLockPending === generation) wakeLockPending = null;
      if (generation !== wakeLockGeneration || !shouldKeepAwake()) {
        sentinel.release().catch(function () {});
        return;
      }
      wakeLock = sentinel;
      if (wakeLockHint) wakeLockHint.textContent = L.wakeLockActive;
      sentinel.addEventListener('release', function () {
        if (wakeLock !== sentinel) return;
        wakeLock = null;
        if (shouldKeepAwake() && wakeLockHint) wakeLockHint.textContent = L.wakeLockUnavailable;
      });
    }).catch(function () {
      if (wakeLockPending === generation) wakeLockPending = null;
      if (generation === wakeLockGeneration && shouldKeepAwake() && wakeLockHint) {
        wakeLockHint.textContent = L.wakeLockUnavailable;
      }
    });
  }
  function initWakeLockSwitch() {
    if (!wakeLockSwitch) return;
    try { wakeLockSwitch.checked = localStorage.getItem(WAKE_LOCK_KEY) !== '0'; }
    catch (e) { wakeLockSwitch.checked = true; }
    if (wakeLockHint) wakeLockHint.textContent = L.wakeLockHint;
    wakeLockSwitch.addEventListener('change', function () {
      try { localStorage.setItem(WAKE_LOCK_KEY, wakeLockSwitch.checked ? '1' : '0'); } catch (e) {}
      if (wakeLockHint) wakeLockHint.textContent = L.wakeLockHint;
      if (wakeLockSwitch.checked) acquireWakeLock();
      else releaseWakeLock();
    });
  }

  // ============================================================
  // 屏幕切换
  // ============================================================
  function showScreen(which) {
    screenPin.classList.toggle('active', which === 'pin');
    screenRec.classList.toggle('active', which === 'rec');
    screenOffline.classList.toggle('active', which === 'offline');
  }

  // ============================================================
  // 模式(toggle / hold)
  // ============================================================
  // ============================================================
  // 电脑落字开关(关闭=只把文字回传手机、不落到电脑光标)
  // ============================================================
  function readInsert() {
    try { return localStorage.getItem(INSERT_KEY) !== '0'; } catch (e) { return true; }
  }
  function writeInsert(v) {
    try { localStorage.setItem(INSERT_KEY, v ? '1' : '0'); } catch (e) {}
  }
  // 把当前开关值发给电脑(仅已连接时生效):进录音屏时同步一次,之后每次切换即时下发。
  function sendInsertConfig() {
    wsSendJSON({ type: 'set_insert', value: insertSwitch ? insertSwitch.checked : true });
  }
  function initInsertSwitch() {
    if (!insertSwitch) return;
    insertSwitch.checked = readInsert();
    insertSwitch.addEventListener('change', function () {
      writeInsert(insertSwitch.checked);
      sendInsertConfig();
    });
  }

  function readMode() {
    var m = null;
    try { m = localStorage.getItem(MODE_KEY); } catch (e) {}
    // 手机明确保存的两种模式优先；首次访问、旧值损坏或存储被禁用时，
    // 跟随 PC 当前默认值。不要把继承值写回存储，否则之后 PC 改设置就失效了。
    if (m === 'hold' || m === 'toggle') return m;
    return window.__OL_DEFAULT_MODE__ === 'hold' ? 'hold' : 'toggle';
  }
  function writeMode(m) {
    mode = m;
    try { localStorage.setItem(MODE_KEY, m); } catch (e) {}
    syncModeUI();
  }
  function syncModeUI() {
    var btns = modeSwitch.querySelectorAll('.mode-btn');
    for (var i = 0; i < btns.length; i++) {
      btns[i].classList.toggle('active', btns[i].getAttribute('data-mode') === mode);
    }
    if (mode === 'hold') {
      recTip.textContent = L.tipHold;
      recordLabel.textContent = recording ? L.labelHoldRec : L.labelHoldIdle;
      recordBtn.style.touchAction = 'none';   // hold 防滚动
    } else {
      recTip.textContent = L.tipToggle;
      recordLabel.textContent = recording ? L.labelToggleRec : L.labelToggleIdle;
      recordBtn.style.touchAction = 'manipulation';
    }
  }

  // 手机手动切换后保存为本机偏好，后续访问继续优先于 PC 默认值。
  modeSwitch.addEventListener('click', function (e) {
    var t = e.target.closest('.mode-btn');
    if (!t) return;
    var m = t.getAttribute('data-mode');
    if (m === mode) return;
    // 录音中切换模式先安全停止(取消本次,避免状态错乱)
    if (recording) cancelRecording();
    writeMode(m);
  });

  // ============================================================
  // 状态文字 / 音量
  // ============================================================
  function setStatus(text, kind) {
    statusText.textContent = text;
    // 每次切状态先清掉图标/三点动效,由调用方(applyStatusKind)按需重新点亮。
    if (statusIcon) statusIcon.hidden = true;
    if (statusDots) statusDots.hidden = true;
    statusBar.classList.remove('is-error', 'is-ok', 'is-work');
    if (kind === 'error') statusBar.classList.add('is-error');
    else if (kind === 'ok') statusBar.classList.add('is-ok');
    else if (kind === 'work') statusBar.classList.add('is-work');
  }
  function setLevel(v) {
    if (typeof v !== 'number' || isNaN(v)) return;
    v = Math.max(0, Math.min(1, v));
    levelBar.style.width = (v * 100).toFixed(1) + '%';
  }

  // 去掉状态文案开头的 emoji 图标(如 '🎤 录音中' → '录音中'),改用 DOM 图标/动效呈现。
  function stripLeadingIcon(s) {
    return String(s).replace(/^\S+\s+/, '');
  }

  // PC 端落字完成后回传的最终文字,显示在状态区下方;开始新一次录音时清空。
  function showResult(text) {
    if (!resultWrap) return;
    if (!text) { clearResult(); return; }
    resultText.textContent = text;
    resultWrap.hidden = false;
  }
  function clearResult() {
    if (!resultWrap) return;
    resultWrap.hidden = true;
    resultText.textContent = '';
    if (resultCopy) {
      resultCopy.classList.remove('copied');
      resultCopy.textContent = L.copy || '复制';
    }
  }

  // done 后过几秒自动回到"准备就绪",方便直接开始下一次,而不是一直停在结果上。
  var readyTimer = null;
  function scheduleReady() {
    if (readyTimer) clearTimeout(readyTimer);
    readyTimer = setTimeout(function () {
      readyTimer = null;
      if (!recording && authed) setStatus(L.ready, null);
    }, 2500);
  }
  // 录音/停止/取消入口都要清掉 readyTimer,否则上一次 done 的回 ready 定时器会迟到
  // 触发,把"识别中…"等新状态错盖成"准备就绪"。
  function clearReadyTimer() {
    if (readyTimer) { clearTimeout(readyTimer); readyTimer = null; }
  }

  // busy 提示的解除定时器:跟踪起来,新状态到来时清除,避免多个 busy 消息叠加定时器
  // 或迟到的定时器覆盖新状态。
  var busyTimer = null;

  // 识别/润色阶段的客户端兜底超时:服务端任何原因不回 done/error(如孤立会话、进程异常)
  // 时,30 秒后显示通用错误并回 ready,防止 UI 永久卡在"识别中…"。
  var workTimer = null;
  function armWorkTimeout() {
    clearWorkTimeout();
    workTimer = setTimeout(function () {
      workTimer = null;
      if (!recording && authed) {
        if (startSent) failRecording('❌ ' + L.errGeneric, true);
        else if (recoverySessionId) requestRecovery();
        else {
          awaitingResult = false;
          updateRecordBtnUI();
          setStatus('❌ ' + L.errGeneric, 'error');
          setLevel(0);
        }
        scheduleReady();
      }
    }, 30000);
  }
  function clearWorkTimeout() {
    if (workTimer) { clearTimeout(workTimer); workTimer = null; }
  }

  // ============================================================
  // WebSocket
  // ============================================================
  function wsSendJSON(obj) {
    if (ws && ws.readyState === 1) {
      try { ws.send(JSON.stringify(obj)); } catch (e) {}
    }
  }

  // 连接看门狗:wss 握手或认证在 12s 内没完成,几乎都是手机没信任电脑证书
  // (iOS Safari 对自签名 wss 不复用页面级证书例外)。与其无限"连接中",不如回到
  // 配对屏给出明确提示,引导用户去信任证书。
  var connectTimer = null;
  function armConnectTimeout() {
    clearConnectTimeout();
    connectTimer = setTimeout(function () {
      connectTimer = null;
      if (!authed) {
        closeWS();
        showScreen('pin');
        showPinError(L.errConnTimeout);
        resetConnectBtn();
      }
    }, 12000);
  }
  function clearConnectTimeout() {
    if (connectTimer) { clearTimeout(connectTimer); connectTimer = null; }
  }

  function connect(pin) {
    lastPin = pin;
    closeWS(); // 清理旧连接
    authed = false;
    busy = false;
    awaitingResult = false;

    var url = 'wss://' + location.host + '/ws';
    try {
      ws = new WebSocket(url);
    } catch (e) {
      showPinError(L.errConnCreate);
      resetConnectBtn();
      return;
    }
    ws.binaryType = 'arraybuffer';
    armConnectTimeout(); // 看门狗:握手/认证迟迟不完成 → 多半是证书没被信任

    ws.onopen = function () {
      // 连上立即握手
      wsSendJSON({ type: 'hello', pin: pin, prefer: mode });
    };

    ws.onmessage = function (ev) {
      if (typeof ev.data !== 'string') return; // 下行只处理文本
      var msg;
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      handleMessage(msg);
    };

    ws.onerror = function () {
      // onerror 后通常紧跟 onclose,统一在 close 里处理 UI
    };

    ws.onclose = function () {
      clearConnectTimeout();
      clearReadyTimer();
      clearWorkTimeout();
      clearRecoveryTimer();
      if (busyTimer) { clearTimeout(busyTimer); busyTimer = null; }
      var wasAuthed = authed;
      authed = false;
      recording = false;
      awaitingResult = false;
      detachHoldEnd();
      resetRemoteStreamState();
      teardownAudio();
      if (wasAuthed) {
        // 已进入录音屏后断开 → 断线屏
        offlineReason.textContent = recoverySessionId ? L.offlineRecording : L.offlineSub;
        showScreen('offline');
      } else {
        // 未认证就关闭(握手被拒/证书不受信任/网络中断)。无论当前是否在配对屏都给出
        // 明确提示 —— 否则(尤其安卓 Chrome 对不受信任的自签名 wss 会立刻 onclose)
        // 用户只看到按钮闪一下变回"连接",完全不知道发生了什么。
        showScreen('pin');
        showPinError(L.errConnFail);
      }
      resetConnectBtn();
    };
  }

  function closeWS() {
    clearConnectTimeout();
    clearReadyTimer();
    clearWorkTimeout();
    clearRecoveryTimer();
    if (busyTimer) { clearTimeout(busyTimer); busyTimer = null; }
    recording = false;
    awaitingResult = false;
    detachHoldEnd();
    resetRemoteStreamState();
    teardownAudio();
    if (ws) {
      ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null;
      try { ws.close(); } catch (e) {}
      ws = null;
    }
  }

  function handleMessage(msg) {
    if (!msg || typeof msg.type !== 'string') return;

    switch (msg.type) {
      case 'auth':
        if (msg.ok) {
          authed = true;
          busy = false;
          clearConnectTimeout();
          writePin(lastPin); // 配对成功 → 记住配对码,刷新后免重输
          enterRecScreen();
          requestRecovery();
        } else {
          authed = false;
          clearPin(); // 配对码失效(错误/锁定)→ 清除,避免下次自动重连又失败
          var reason = msg.reason === 'locked' ? L.errPinLocked : L.errPinWrong;
          closeWS();
          showScreen('pin');
          showPinError(reason);
          resetConnectBtn();
        }
        break;

      case 'status':
        applyStatusKind(msg);
        break;

      case 'started':
        handleStarted(msg.sessionId, msg.recoveryKey);
        break;

      case 'recovery':
        handleRecovery(msg);
        break;

      case 'level':
        setLevel(msg.value);
        break;

      case 'busy':
        clearWorkTimeout();
        busy = true;
        recording = false;
        awaitingResult = false;
        resetRemoteStreamState();
        teardownAudioCapture(); // 停止采集但保留 ctx
        updateRecordBtnUI();
        setStatus(fmt(L.busy, { reason: msg.reason || L.busyDefault }), 'error');
        // 短暂后解除忙态,允许重试。定时器存入 busyTimer 跟踪,重入时先清,避免叠加。
        if (busyTimer) clearTimeout(busyTimer);
        busyTimer = setTimeout(function () {
          busyTimer = null;
          busy = false;
          updateRecordBtnUI();
          if (!recording) setStatus(L.ready, null);
        }, 1500);
        break;

      case 'result':
        // 电脑落字完成后回传的最终文字,显示给手机用户看本次识别结果。
        showResult(msg.text);
        awaitingResult = false;
        clearRecoveryTimer();
        updateRecordBtnUI();
        break;
    }
  }

  function applyStatusKind(msg) {
    // 真实状态到来即解除 busy 兜底定时,避免它迟到触发把新状态错盖成"准备就绪"。
    if (busyTimer) {
      clearTimeout(busyTimer); busyTimer = null;
      busy = false;
      updateRecordBtnUI();
    }
    switch (msg.kind) {
      case 'recording':
        setStatus(stripLeadingIcon(L.statusRecording), 'work');
        break;
      case 'transcribing':
        if (recording) {
          recording = false;
          detachHoldEnd();
          resetRemoteStreamState();
          teardownAudioCapture();
        }
        awaitingResult = true;
        updateRecordBtnUI();
        setStatus(stripLeadingIcon(L.statusTranscribing), 'work');
        if (statusDots) statusDots.hidden = false; // 识别中:三点加载动效
        armWorkTimeout(); // 工作状态续上兜底超时,防止服务端中途无响应卡死
        break;
      case 'polishing':
        setStatus(L.statusPolishing, 'work'); // 润色保留 ✨
        armWorkTimeout(); // 同上
        break;
      case 'done':
        awaitingResult = false;
        updateRecordBtnUI();
        clearWorkTimeout(); // 正常收尾,解除兜底超时
        var n = (typeof msg.insertedChars === 'number') ? msg.insertedChars : 0;
        setStatus(stripLeadingIcon(fmt(L.statusDone, { n: n })), 'ok');
        if (statusIcon) { statusIcon.src = '/done.png'; statusIcon.hidden = false; } // 完成:对勾图
        setLevel(0);
        scheduleReady();
        break;
      case 'error':
        awaitingResult = false;
        updateRecordBtnUI();
        clearWorkTimeout(); // 服务端已明确报错,解除兜底超时
        if (recording || startSent) failRecording('❌ ' + (msg.message || L.errGeneric), true);
        else {
          resetRemoteStreamState();
          setStatus('❌ ' + (msg.message || L.errGeneric), 'error');
          setLevel(0);
        }
        break;
      default:
        if (msg.message) setStatus(msg.message, null);
    }
  }

  // ============================================================
  // 屏幕状态判断辅助
  // ============================================================
  function isPinScreen() { return screenPin.classList.contains('active'); }

  function enterRecScreen() {
    showPinError('');
    showScreen('rec');
    syncModeUI();
    updateRecordBtnUI();
    setStatus(L.ready, null);
    setLevel(0);
    sendInsertConfig(); // 进录音屏时把「电脑落字」开关同步给电脑
  }

  // ============================================================
  // PIN 屏交互
  // ============================================================
  pinInput.addEventListener('input', function () {
    // 仅保留数字
    var v = pinInput.value.replace(/\D+/g, '').slice(0, 6);
    if (v !== pinInput.value) pinInput.value = v;
    showPinError('');
  });
  pinInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') doConnect();
  });
  btnConnect.addEventListener('click', doConnect);

  function doConnect() {
    var pin = (pinInput.value || '').replace(/\D+/g, '');
    if (pin.length !== 6) {
      showPinError(L.errPinFormat);
      return;
    }
    showPinError('');
    btnConnect.disabled = true;
    btnConnect.textContent = L.btnConnecting;
    connect(pin);
  }

  function showPinError(text) {
    if (!text) {
      pinError.hidden = true;
      pinError.textContent = '';
    } else {
      pinError.hidden = false;
      pinError.textContent = text;
    }
  }
  function resetConnectBtn() {
    btnConnect.disabled = false;
    btnConnect.textContent = L.btnConnect;
  }

  // 重新连接
  btnReconnect.addEventListener('click', function () {
    showScreen('pin');
    showPinError('');
    resetConnectBtn();
    var p = lastPin || readPin();
    if (p) {
      pinInput.value = p;
      doConnect(); // 有配对码直接重连,省去再点一次
    }
  });

  // 复制证书下载链接 —— 方便换个浏览器打开,或发给自己。
  function fallbackCopyText(text, cb) {
    try {
      var ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      document.body.removeChild(ta);
      if (cb) cb();
    } catch (e) {}
  }
  if (copyCertBtn) {
    copyCertBtn.addEventListener('click', function () {
      var url = location.origin + '/cert.mobileconfig';
      var ok = function () {
        copyCertBtn.textContent = L.helpCopied;
        setTimeout(function () { copyCertBtn.textContent = L.helpCopyLink; }, 1500);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(url).then(ok, function () { fallbackCopyText(url, ok); });
      } else {
        fallbackCopyText(url, ok);
      }
    });
  }

  // 结果文字「一键复制」：优先 navigator.clipboard(需安全上下文,本页是 HTTPS),
  // 失败或旧浏览器回退 execCommand(兼容性高,见 fallbackCopyText)。
  if (resultCopy) {
    resultCopy.addEventListener('click', function () {
      var text = resultText.textContent || '';
      if (!text) return;
      var done = function () {
        resultCopy.classList.add('copied');
        resultCopy.textContent = L.copied || '已复制 ✓';
        setTimeout(function () {
          resultCopy.classList.remove('copied');
          resultCopy.textContent = L.copy || '复制';
        }, 1500);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () { fallbackCopyText(text, done); });
      } else {
        fallbackCopyText(text, done);
      }
    });
  }

  // ============================================================
  // 录音按钮交互(toggle / hold)
  // ============================================================
  function updateRecordBtnUI() {
    recordBtn.classList.toggle('recording', recording);
    recordBtn.classList.toggle('busy', (busy || awaitingResult) && !recording);
    recordBtn.disabled = (busy || awaitingResult) && !recording;
    if (recording) {
      recordLabel.textContent = (mode === 'hold') ? L.labelHoldRec : L.labelToggleRec;
    } else {
      recordLabel.textContent = (mode === 'hold') ? L.labelHoldIdle : L.labelToggleIdle;
    }
  }

  // toggle 模式:click 切换
  recordBtn.addEventListener('click', function () {
    if (mode !== 'toggle') return;
    if (!authed || busy || awaitingResult) return;
    if (recording) stopRecording();
    else startRecording();
  });

  // hold 模式:按下开始;松开/取消结束。
  // 关键:用 document 级监听兜底"松开"事件。移动端 setPointerCapture 在动画/重排/
  // 系统权限弹窗时可能丢失,导致 recordBtn 自身的 pointerup 收不到 —— 表现为"手已
  // 松开却还在录音,得再点一下才停"。改为按下时在 document 上挂一次性的 pointerup/
  // pointercancel,无论指针最终在哪释放都能结束录音。
  var holdEndHandler = null;
  function attachHoldEnd() {
    if (holdEndHandler) return;
    holdEndHandler = function () {
      if (recording) stopRecording(); // stopRecording 内部会 detachHoldEnd
      else detachHoldEnd();
    };
    document.addEventListener('pointerup', holdEndHandler, true);
    document.addEventListener('pointercancel', holdEndHandler, true);
  }
  function detachHoldEnd() {
    if (!holdEndHandler) return;
    document.removeEventListener('pointerup', holdEndHandler, true);
    document.removeEventListener('pointercancel', holdEndHandler, true);
    holdEndHandler = null;
  }

  recordBtn.addEventListener('pointerdown', function (e) {
    if (mode !== 'hold') return;
    if (!authed || busy || awaitingResult) return;
    e.preventDefault();
    attachHoldEnd();
    if (!recording) startRecording();
  });

  // ============================================================
  // 录音流程
  // ============================================================
  // 给可能"永久 pending"的 Promise 兜底超时。移动端 audioCtx.resume() / getUserMedia()
  // 在息屏/切后台/被占用时可能既不 resolve 也不 reject,整条 ensureAudio 链就永久卡住 ——
  // start 指令发不出去、电脑端不弹胶囊,H5 一直停在"正在准备麦克风…"。超时即判失败,复位
  // 状态并提示重试,而不是无限等待。
  function withTimeout(promise, ms, tag) {
    return new Promise(function (resolve, reject) {
      var timer = setTimeout(function () {
        var err = new Error(tag || 'TIMEOUT');
        err.name = tag || 'TIMEOUT';
        reject(err);
      }, ms);
      promise.then(
        function (v) { clearTimeout(timer); resolve(v); },
        function (e) { clearTimeout(timer); reject(e); }
      );
    });
  }

  function startRecording() {
    if (recording || startSent || awaitingResult) return;
    if (!ws || ws.readyState !== 1) {
      setStatus(L.connLost, 'error');
      return;
    }
    // 先乐观置态,保证 iOS 在手势同步栈内 resume()
    recording = true;
    clearRecoveryTimer();
    acquireWakeLock();
    resetRemoteStreamState();
    clearReadyTimer();  // 防止上一次 done 的回 ready 定时器迟到覆盖本次状态
    clearWorkTimeout(); // 新一次录音开始,作废上一轮的识别兜底超时
    updateRecordBtnUI();
    setStatus(L.preparingMic, 'work');
    clearResult(); // 清掉上一次的识别结果,避免新录音时还显示旧文字

    withTimeout(ensureAudio(), MIC_PREP_TIMEOUT_MS, 'TIMEOUT')
      .then(function () {
        if (!recording) {
          // 期间已被取消/松手
          teardownAudioCapture();
          return;
        }
        wsSendJSON({ type: 'start' });
        startSent = true; // start 已发出,stopRecording 才需要配对发 stop
        setStatus(L.preparingBackend, 'work');
      })
      .catch(function (err) {
        recording = false;
        resetRemoteStreamState();
        // 超时多半是 audioCtx 卡死(resume 永不 settle),彻底重建,否则下次重试会继续卡在
        // 同一个坏 ctx 上;非超时错误只需停采集链。
        if (err && err.name === 'TIMEOUT') resetAudioContext();
        else teardownAudioCapture();
        updateRecordBtnUI();
        setStatus(err && err.name === 'TIMEOUT' ? L.micTimeout : micErrorText(err), 'error');
      });
  }

  function stopRecording() {
    detachHoldEnd();
    if (!recording) return;
    clearReadyTimer(); // 防止迟到的回 ready 定时器覆盖"识别中…"
    recording = false;
    updateRecordBtnUI();
    teardownAudioCapture();
    // start 还没发出(hold 按下后立即松手,ensureAudio 尚未完成)→ 按本地取消处理:
    // 不发孤立 stop,否则 PC 无对应会话、不回 done/error,UI 会永久卡在"识别中…"。
    if (!startSent) {
      resetRemoteStreamState();
      setStatus(L.ready, null);
      setLevel(0);
      return;
    }
    awaitingResult = true;
    updateRecordBtnUI();
    if (remoteSessionId) {
      wsSendJSON({ type: 'stop' });
      resetRemoteStreamState();
      enterTranscribing();
    } else {
      // ACK 未到：先保留首段 PCM，ACK 后按序 flush，再把 stop 排在音频帧之后。
      finishAfterStarted = 'stop';
      setStatus(L.preparingBackend, 'work');
      setLevel(0);
      armWorkTimeout();
    }
  }

  function cancelRecording() {
    detachHoldEnd();
    awaitingResult = false;
    clearRecoveryTimer();
    saveRecoverySession('');
    if (!recording && !startSent) {
      teardownAudioCapture();
      resetRemoteStreamState();
      return;
    }
    clearReadyTimer();
    clearWorkTimeout();
    recording = false;
    updateRecordBtnUI();
    teardownAudioCapture();
    if (startSent) {
      wsSendJSON({ type: 'cancel' });
      if (remoteSessionId) resetRemoteStreamState();
      else {
        finishAfterStarted = 'cancel';
        clearPendingPcm();
      }
    } else {
      resetRemoteStreamState();
    }
    setStatus(L.cancelled, null);
    setLevel(0);
  }

  function micErrorText(err) {
    var name = err && err.name ? err.name : '';
    if (name === 'NotAllowedError' || name === 'SecurityError') {
      return L.micDenied;
    }
    if (name === 'NotFoundError' || name === 'OverconstrainedError') {
      return L.micNotFound;
    }
    if (name === 'NotReadableError') {
      return L.micBusy;
    }
    return fmt(L.micUnknown, { name: name ? '(' + name + ')' : '' });
  }

  // ============================================================
  // 音频:获取设备 + 建立采集链
  // ============================================================
  // 确保 AudioContext / getUserMedia / 采集节点就绪并开始推流。
  // 必须在用户手势调用栈内(startRecording 由手势触发)。
  function ensureAudio() {
    // 不支持 getUserMedia
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      return Promise.reject(new Error('UNSUPPORTED:浏览器不支持录音,请升级或换浏览器'));
    }

    // 1) AudioContext(iOS 需手势内 resume)
    if (!audioCtx) {
      var AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) {
        return Promise.reject(new Error('UNSUPPORTED:浏览器不支持录音,请升级或换浏览器'));
      }
      audioCtx = new AC();
      audioCtx.onstatechange = function () {
        if (audioCtx && recording && startSent && audioCtx.state !== 'running') {
          interruptRecording();
        }
      };
    }

    // 注意:iOS Safari 来电/Siri 后 ctx 处于私有的 'interrupted' 状态,只判 'suspended'
    // 不命中,会导致录音静默无声 —— 凡是非 running 都尝试 resume。
    var resumeP = (audioCtx.state !== 'running')
      ? audioCtx.resume().catch(function () {})
      : Promise.resolve();

    return resumeP
      .then(function () {
        // 2) 麦克风流(已存在则复用)
        if (mediaStream) return mediaStream;
        // 捕获当前代际:迟到 resolve 时若代际已变(超时重置/断线释放),停掉轨道并放弃,
        // 避免泄漏麦克风或覆盖重试成功的新流。
        var gen = audioGen;
        return navigator.mediaDevices.getUserMedia({
          audio: {
            channelCount: 1,
            echoCancellation: true,
            noiseSuppression: true,
            autoGainControl: true
          },
          video: false
        }).then(function (stream) {
          if (gen !== audioGen) {
            try { stream.getTracks().forEach(function (t) { t.stop(); }); } catch (e) {}
            return null; // 交给下一步判空直接放弃
          }
          mediaStream = stream;
          stream.getTracks().forEach(function (track) {
            track.onended = function () { if (recording) interruptRecording(); };
          });
          return stream;
        });
      })
      .then(function (stream) {
        // 3) 建立采集图(若已建好则跳过)。audioCtx 可能在准备超时后被 resetAudioContext
        // 置空(本次 getUserMedia 迟到 resolve),此时直接放弃,避免对 null ctx 建图报错。
        if (sourceNode || !audioCtx || !stream) return;
        sourceNode = audioCtx.createMediaStreamSource(stream);
        return buildCaptureGraph();
      });
  }

  // 建立 AudioWorklet(优先)或 ScriptProcessor(兜底)
  function buildCaptureGraph() {
    var inSr = audioCtx.sampleRate || 48000;

    // 优先 AudioWorklet
    if (audioCtx.audioWorklet && typeof AudioWorkletNode !== 'undefined') {
      return loadWorklet()
        .then(function () {
          workletNode = new AudioWorkletNode(audioCtx, 'ol-pcm-worklet', {
            numberOfInputs: 1,
            numberOfOutputs: 0,
            channelCount: 1,
            processorOptions: { inSr: inSr, targetSr: TARGET_SR }
          });
          workletNode.port.onmessage = function (e) {
            // e.data 是已转换好的 Int16 LE ArrayBuffer
            sendAudio(e.data);
          };
          sourceNode.connect(workletNode);
          usingWorklet = true;
        })
        .catch(function () {
          // worklet 加载失败 → 回退 ScriptProcessor
          usingWorklet = false;
          buildScriptProcessor(inSr);
        });
    }

    // 无 audioWorklet:直接兜底
    usingWorklet = false;
    buildScriptProcessor(inSr);
    return Promise.resolve();
  }

  // ---- AudioWorklet processor(字符串 → Blob URL 加载) ----
  function loadWorklet() {
    if (workletUrl) return audioCtx.audioWorklet.addModule(workletUrl);

    var code =
      'class OlPcmWorklet extends AudioWorkletProcessor {' +
      '  constructor(o){' +
      '    super();' +
      '    var p=(o&&o.processorOptions)||{};' +
      '    this.inSr=p.inSr||sampleRate;' +
      '    this.targetSr=p.targetSr||16000;' +
      '    this.ratio=this.inSr/this.targetSr;' +
      '    this.phase=0;' +       // 当前小数相位
      '    this.last=0;' +        // 上一块最后一个样本(用于跨块拼接)
      '    this.hasLast=false;' +
      '  }' +
      '  process(inputs){' +
      '    var ch=inputs[0]&&inputs[0][0];' +
      '    if(!ch||ch.length===0){return true;}' +
      '    var ratio=this.ratio;' +
      '    var phase=this.phase;' +
      '    var prev=this.last;' +
      '    var hasPrev=this.hasLast;' +
      '    var n=ch.length;' +
      // 估算输出样本数上界
      '    var outCap=Math.ceil((n+1)/ratio)+2;' +
      '    var pcm=new ArrayBuffer(outCap*2);' +
      '    var dv=new DataView(pcm);' +
      '    var oi=0;' +
      // 线性插值:phase 以"输入样本"为单位推进,step=inSr/16000
      // i=floor(phase),frac=phase-i;a=样本[i],b=样本[i+1]
      // 跨块时 i 可能为 -1,用 prev 作为 a。
      '    while(true){' +
      '      var i=Math.floor(phase);' +
      '      var frac=phase-i;' +
      '      var a,b;' +
      '      if(i+1>=n){break;}' +            // 需要 i 和 i+1 都在块内(或 a 用 prev)
      '      if(i<0){' +
      '        if(!hasPrev){phase+=ratio;continue;}' +
      '        a=prev;b=ch[0];' +
      '      }else{' +
      '        a=ch[i];b=ch[i+1];' +
      '      }' +
      '      var s=a+(b-a)*frac;' +
      '      if(s>1)s=1;else if(s<-1)s=-1;' +
      '      dv.setInt16(oi*2, (s*32767)|0, true);' +
      '      oi++;' +
      '      phase+=ratio;' +
      '    }' +
      // 保留余数:把 phase 拉回到相对下一块起点
      '    this.phase=phase-n;' +
      '    this.last=ch[n-1];' +
      '    this.hasLast=true;' +
      '    if(oi>0){' +
      '      var out=pcm.slice(0,oi*2);' +
      '      this.port.postMessage(out,[out]);' +
      '    }' +
      '    return true;' +
      '  }' +
      '}' +
      'registerProcessor("ol-pcm-worklet", OlPcmWorklet);';

    workletUrl = URL.createObjectURL(new Blob([code], { type: 'application/javascript' }));
    return audioCtx.audioWorklet.addModule(workletUrl);
  }

  // ---- ScriptProcessor 兜底 ----
  function buildScriptProcessor(inSr) {
    scriptNode = audioCtx.createScriptProcessor(4096, 1, 1);
    resampleState.phase = 0;
    resampleState.last = 0;
    resampleState.hasLast = false;

    scriptNode.onaudioprocess = function (e) {
      if (!recording) return;
      var input = e.inputBuffer.getChannelData(0);
      var buf = resampleToInt16LE(input, inSr);
      if (buf && buf.byteLength) sendAudio(buf);
    };
    // ScriptProcessor 需连到 destination 才会触发(用静音增益避免回放)
    sourceNode.connect(scriptNode);
    var silent = audioCtx.createGain();
    silent.gain.value = 0;
    scriptNode.connect(silent);
    silent.connect(audioCtx.destination);
    scriptNode._silentGain = silent;
  }

  // 主线程线性插值重采样(给 ScriptProcessor 用),逻辑与 worklet 一致
  function resampleToInt16LE(ch, inSr) {
    var ratio = inSr / TARGET_SR;
    var phase = resampleState.phase;
    var prev = resampleState.last;
    var hasPrev = resampleState.hasLast;
    var n = ch.length;
    if (n === 0) return null;

    var outCap = Math.ceil((n + 1) / ratio) + 2;
    var pcm = new ArrayBuffer(outCap * 2);
    var dv = new DataView(pcm);
    var oi = 0;

    while (true) {
      var i = Math.floor(phase);
      var frac = phase - i;
      var a, b;
      if (i + 1 >= n) break;
      if (i < 0) {
        if (!hasPrev) { phase += ratio; continue; }
        a = prev; b = ch[0];
      } else {
        a = ch[i]; b = ch[i + 1];
      }
      var s = a + (b - a) * frac;
      if (s > 1) s = 1; else if (s < -1) s = -1;
      dv.setInt16(oi * 2, (s * 32767) | 0, true);
      oi++;
      phase += ratio;
    }

    resampleState.phase = phase - n;
    resampleState.last = ch[n - 1];
    resampleState.hasLast = true;

    return oi > 0 ? pcm.slice(0, oi * 2) : null;
  }

  function clearPendingPcm() {
    pendingPcm = [];
    pendingPcmBytes = 0;
  }

  function resetRemoteStreamState() {
    startSent = false;
    remoteSessionId = '';
    remoteSequence = 0;
    finishAfterStarted = '';
    clearPendingPcm();
  }

  function enterTranscribing() {
    setStatus(stripLeadingIcon(L.statusTranscribing), 'work');
    if (statusDots) statusDots.hidden = false;
    setLevel(0);
    armWorkTimeout();
  }

  function failRecording(message, notifyBackend) {
    var waitingForAck = startSent && !remoteSessionId;
    recording = false;
    awaitingResult = false;
    detachHoldEnd();
    teardownAudioCapture();
    if (notifyBackend && startSent) wsSendJSON({ type: 'cancel' });
    if (waitingForAck) {
      finishAfterStarted = 'cancel';
      clearPendingPcm();
    } else {
      resetRemoteStreamState();
    }
    updateRecordBtnUI();
    setStatus(message, 'error');
    setLevel(0);
  }

  function sendRemoteFrame(buf) {
    if (!ws || ws.readyState !== 1 || !remoteSessionId) return false;
    try {
      ws.send(buildAudioFrame(remoteSessionId, remoteSequence, buf));
      remoteSequence++;
      return true;
    } catch (e) {
      return false;
    }
  }

  function flushPendingPcm() {
    var queued = pendingPcm;
    clearPendingPcm();
    for (var i = 0; i < queued.length; i++) {
      if (!sendRemoteFrame(queued[i])) return false;
    }
    return true;
  }

  function handleStarted(sessionId, key) {
    if (!startSent) {
      clearPendingPcm();
      return;
    }
    if (finishAfterStarted === 'cancel') {
      resetRemoteStreamState();
      updateRecordBtnUI();
      return;
    }
    if (remoteSessionId) return;
    if (typeof sessionId !== 'string' ||
        !/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(sessionId)) {
      failRecording('❌ ' + L.errGeneric, true);
      return;
    }

    remoteSessionId = sessionId;
    saveRecoverySession(sessionId, key);
    remoteSequence = 0;
    if (!flushPendingPcm()) {
      interruptRecording();
      return;
    }
    if (finishAfterStarted === 'stop') {
      wsSendJSON({ type: 'stop' });
      resetRemoteStreamState();
      enterTranscribing();
    } else if (recording) {
      setStatus(stripLeadingIcon(L.statusRecording), 'work');
    } else {
      wsSendJSON({ type: 'cancel' });
      resetRemoteStreamState();
    }
  }

  // 发送二进制音频帧；start ACK 前最多缓存 128 KiB，避免冷启动吞掉首词。
  function sendAudio(buf) {
    if (!recording || !buf || !buf.byteLength) return;
    if (!ws || ws.readyState !== 1) {
      interruptRecording();
      return;
    }
    if (remoteSessionId) {
      if (!sendRemoteFrame(buf)) interruptRecording();
      else updateLocalLevel(buf);
      return;
    }
    if (pendingPcmBytes + buf.byteLength > PCM_QUEUE_MAX_BYTES) {
      failRecording(L.pcmQueueOverflow, true);
      return;
    }
    pendingPcm.push(buf);
    pendingPcmBytes += buf.byteLength;
    updateLocalLevel(buf);
  }

  function buildAudioFrame(sessionId, sequence, pcm) {
    var hex = sessionId.replace(/-/g, '');
    if (!/^[0-9a-fA-F]{32}$/.test(hex)) throw new Error('invalid session id');
    var frame = new ArrayBuffer(28 + pcm.byteLength);
    var view = new DataView(frame);
    view.setUint8(0, 0x4f); view.setUint8(1, 0x4c);
    view.setUint8(2, 0x32); view.setUint8(3, 0x30);
    for (var i = 0; i < 16; i++) view.setUint8(4 + i, parseInt(hex.slice(i * 2, i * 2 + 2), 16));
    var high = Math.floor(sequence / 0x100000000);
    var low = sequence >>> 0;
    view.setUint32(20, high, false);
    view.setUint32(24, low, false);
    new Uint8Array(frame, 28).set(new Uint8Array(pcm));
    return frame;
  }

  // 本地音量可视化:直接用即将上传的 Int16 PCM 算 RMS。远程模式下 PC 端没有麦克风
  // 电平源(不开本地 cpal),所以电平条由手机端自己的音频驱动 —— 实时,且不依赖后端事件。
  var lastLevelAt = 0;
  function updateLocalLevel(buf) {
    var now = (window.performance && performance.now) ? performance.now() : 0;
    if (now && now - lastLevelAt < 50) return; // 限到 ~20Hz,避免过度刷新 DOM
    lastLevelAt = now;
    var n = buf.byteLength >> 1;
    if (n === 0) return;
    var dv = new DataView(buf);
    var sum = 0;
    for (var i = 0; i < n; i++) {
      var s = dv.getInt16(i * 2, true) / 32768;
      sum += s * s;
    }
    var rms = Math.sqrt(sum / n);
    setLevel(Math.min(1, rms * 3.5)); // 适度放大,让正常说话有明显跳动
  }

  // ============================================================
  // 音频清理
  // ============================================================
  // 仅停止"采集/推流"(断开节点),保留 audioCtx & mediaStream 以便快速重启。
  function teardownAudioCapture() {
    releaseWakeLock();
    if (wakeLockHint) wakeLockHint.textContent = L.wakeLockHint;
    try { if (workletNode) { workletNode.port.onmessage = null; workletNode.disconnect(); } } catch (e) {}
    workletNode = null;

    try {
      if (scriptNode) {
        scriptNode.onaudioprocess = null;
        scriptNode.disconnect();
        if (scriptNode._silentGain) {
          try { scriptNode._silentGain.disconnect(); } catch (e2) {}
        }
      }
    } catch (e) {}
    scriptNode = null;

    try { if (sourceNode) sourceNode.disconnect(); } catch (e) {}
    // sourceNode 置空,下次 ensureAudio 重新从 stream 创建
    sourceNode = null;

    // 复位兜底重采样状态
    resampleState.phase = 0;
    resampleState.last = 0;
    resampleState.hasLast = false;
  }

  // 彻底释放(断线时):停止麦克风轨道并关闭 ctx。
  function teardownAudio() {
    audioGen++; // 代际推进:作废所有在途的 getUserMedia 迟到回调
    teardownAudioCapture();
    if (mediaStream) {
      try {
        var tracks = mediaStream.getTracks();
        for (var i = 0; i < tracks.length; i++) tracks[i].stop();
      } catch (e) {}
      mediaStream = null;
    }
    // 不强行 close ctx(部分浏览器再次 new 较慢);仅在确实需要时挂起
    if (audioCtx && audioCtx.state === 'running') {
      try { audioCtx.suspend(); } catch (e) {}
    }
  }

  // 准备超时后的硬复位:停麦克风轨道并彻底关闭 audioCtx,使下次 ensureAudio 从零重建。
  // 与 teardownAudio 的区别:这里 close 并置空 audioCtx —— 超时根因往往是 ctx 自身坏掉
  // (resume 永不 settle),保留它只会让下次继续卡。
  function resetAudioContext() {
    audioGen++; // 代际推进:作废所有在途的 getUserMedia 迟到回调
    teardownAudioCapture();
    if (mediaStream) {
      try {
        var tracks = mediaStream.getTracks();
        for (var i = 0; i < tracks.length; i++) tracks[i].stop();
      } catch (e) {}
      mediaStream = null;
    }
    if (audioCtx) {
      try { audioCtx.close(); } catch (e) {}
      audioCtx = null;
    }
  }

  // ============================================================
  // 息屏和切后台结束本段录音，保留电脑已收到的部分。
  // ============================================================
  function interruptRecording() {
    var hadStarted = startSent;
    if (recording) stopRecording();
    else if (startSent && remoteSessionId) {
      wsSendJSON({ type: 'stop' });
      resetRemoteStreamState();
      awaitingResult = true;
      teardownAudioCapture();
      updateRecordBtnUI();
    }
    // 系统中断后释放旧轨道，下一次由用户开始录音时重新获取麦克风。
    teardownAudio();
    if (hadStarted) setStatus(L.interrupted, 'work');
  }
  document.addEventListener('visibilitychange', function () {
    if (document.hidden) {
      if (recording) interruptRecording();
      releaseWakeLock();
      clearRecoveryTimer();
      clearWorkTimeout();
    } else {
      if (recording) acquireWakeLock();
      if (authed) requestRecovery();
      else if (!ws || ws.readyState > 1) {
        var pin = readPin();
        if (pin) connect(pin);
      }
    }
  });
  window.addEventListener('pagehide', function () {
    if (recording) interruptRecording();
    releaseWakeLock();
  });

  // ============================================================
  // 初始化
  // ============================================================
  function init() {
    // iOS Safari 怪癖兜底：页面"首次加载"后,页面内 wss 的证书信任不生效 —— 首次连接
    // 会卡在 TLS 握手→超时,手动刷新一次就好(已用日志证实:首次 TCP 到了却不升级,刷新
    // 后立刻 WS 升级成功)。这里把那一下"刷新"自动化:每个浏览器会话首次加载时静默
    // reload 一次,之后再初始化+自动连接,wss 握手就能成功。sessionStorage 标记保证只刷
    // 一次、不会死循环;手动刷新(同标签)不会重复触发,新标签/重开才会再刷。
    var reloadedOnce = false;
    try { reloadedOnce = sessionStorage.getItem('ol_reloaded_once') === '1'; } catch (e) {}
    if (!reloadedOnce) {
      // 写后立即读回校验:sessionStorage 被禁用(写入抛异常/写不进去)时标记永远落不下,
      // 若仍 reload 会无限循环刷新 —— 校验失败就放弃刷新,直接继续初始化。
      var marked = false;
      try {
        sessionStorage.setItem('ol_reloaded_once', '1');
        marked = sessionStorage.getItem('ol_reloaded_once') === '1';
      } catch (e) {}
      if (marked) {
        location.reload();
        return;
      }
    }

    applyStaticI18n();
    syncModeUI();
    initInsertSwitch();
    initWakeLockSwitch();
    showScreen('pin');
    showPinError('');
    // 上次成功的配对码 → 自动填充并重连,刷新/重开页面免再输一次
    var saved = readPin();
    if (saved) {
      pinInput.value = saved;
      doConnect();
    } else {
      // 自动聚焦 PIN(部分移动端会被策略拦截,忽略失败)
      setTimeout(function () { try { pinInput.focus(); } catch (e) {} }, 200);
    }
  }

  init();
})();
