// Shared SIWC copy; backend errors contain only safe diagnostic codes.
export const siwcZhCN = {
  description:
    '符合条件的文本润色和翻译使用你的 ChatGPT 计划或积分余额。ASR 仍使用原渠道；额度不足时不会自动切换到 API Key。',
  account: 'ChatGPT 账号',
  cancel: '取消登录',
  enableUsage: '授权计划用量',
  reauthorize: '重新授权',
  addAccount: '添加账号或工作区',
  disconnect: '断开连接',
  manageUsage: '管理用量',
  retry: '重试计划用量',
  model: '润色模型',
  selectModel: '选择账号可用模型',
  modelUnavailable: '模型 {{model}} 当前不可用，请重新选择。',
  noModels: '该账号暂时没有可显示的模型。',
  refreshModels: '刷新模型',
  test: '测试连接',
  testHint: '获取模型列表不执行推理；测试连接会使用少量 ChatGPT 计划用量。',
  testSuccess: '连接测试成功',
  remoteUnconfirmed: '已在本机断开连接，但未确认远端撤销。请到 ChatGPT 设置中断开此应用。',
  welcome: '正在使用你的 ChatGPT 计划',
  gotIt: '知道了',
  failed: '请求未完成，请检查连接或重新授权。',
  states: {
    signed_out: '尚未连接 ChatGPT',
    authorizing: '请在系统浏览器中完成授权',
    connected: '正在使用 ChatGPT 计划',
    usage_disabled: '已登录，尚未授权计划用量',
    usage_limited: '已达到计划或此应用的用量限制',
    reauthorization_required: '需要重新登录 ChatGPT',
  },
  errors: {
    sign_in_required: '请重新登录 ChatGPT。',
    reauthorization_required: '请重新登录 ChatGPT。',
    session_changed: '请重新登录 ChatGPT。',
    refresh_interrupted: '请重新登录 ChatGPT。',
    invalid_grant: '请重新登录 ChatGPT。',
    invalid_refresh_token: '请重新登录 ChatGPT。',
    token_expired: '请重新登录 ChatGPT。',
    refresh_token_expired: '请重新登录 ChatGPT。',
    refresh_token_invalidated: '请重新登录 ChatGPT。',
    refresh_token_reused: '请重新登录 ChatGPT。',
    unauthorized: '请重新登录 ChatGPT。',
    subscription_sharing_invalid_user: '请重新登录 ChatGPT。',
    usage_disabled: '请授权 OpenLess 使用 ChatGPT 计划用量。',
    access_denied: '请授权 OpenLess 使用 ChatGPT 计划用量。',
    chatpass_v2_scope_not_authorized: '请授权 OpenLess 使用 ChatGPT 计划用量。',
    chatpass_v2_invalid_authorization_context: '请授权 OpenLess 使用 ChatGPT 计划用量。',
    subscription_sharing_usage_limit_exceeded: '已达到计划或此应用的用量限制，请前往“管理用量”。',
    rate_limited: '已达到计划或此应用的用量限制，请前往“管理用量”。',
    subscription_sharing_user_not_eligible: '当前账号、工作区或地区不符合计划用量要求。',
    forbidden: '当前账号、工作区或地区不符合计划用量要求。',
    network_failed: '暂时无法连接服务，请稍后重试。你的账号信息已保留。',
    unavailable: '暂时无法连接服务，请稍后重试。你的账号信息已保留。',
    subscription_sharing_usage_unavailable: '暂时无法连接服务，请稍后重试。你的账号信息已保留。',
    subscription_sharing_user_unavailable: '暂时无法连接服务，请稍后重试。你的账号信息已保留。',
    id_token_invalid: '无法验证本次授权，请重新发起登录。',
    id_token_missing: '无法验证本次授权，请重新发起登录。',
    issuer_mismatch: '无法验证本次授权，请重新发起登录。',
    client_mismatch: '无法验证本次授权，请重新发起登录。',
    identity_mismatch: '无法验证本次授权，请重新发起登录。',
    callback_invalid: '无法验证本次授权，请重新发起登录。',
    state_mismatch: '无法验证本次授权，请重新发起登录。',
    registration_incomplete: '无法验证本次授权，请重新发起登录。',
    discovery_invalid: '无法验证本次授权，请重新发起登录。',
    jwks_invalid: '无法验证本次授权，请重新发起登录。',
    token_response_invalid: '无法验证本次授权，请重新发起登录。',
    storage_failed: '无法安全读取或保存登录凭据，请检查系统安全存储后重试。',
    storage_corrupt: '无法安全读取或保存登录凭据，请检查系统安全存储后重试。',
    storage_unavailable: '无法安全读取或保存登录凭据，请检查系统安全存储后重试。',
    storage_full: '无法安全读取或保存登录凭据，请检查系统安全存储后重试。',
    cancelled: '本次操作已取消。',
    authorization_timeout: '授权已超时，请重新登录。',
    subscription_sharing_unsupported_capability: '所选模型或请求能力不可用，请刷新模型并重新选择。',
    subscription_sharing_route_not_supported: '所选模型或请求能力不可用，请刷新模型并重新选择。',
    models_invalid: '所选模型或请求能力不可用，请刷新模型并重新选择。',
    empty_response: '所选模型或请求能力不可用，请刷新模型并重新选择。',
    busy: '另一个账号操作正在进行，请稍后重试。',
    browser_failed: '无法打开系统浏览器，请重试。',
    callback_unavailable: '无法打开系统浏览器，请重试。',
  },
};

export const siwcEn = {
  description:
    'Eligible text polishing and translation use your ChatGPT plan or credits. ASR keeps its existing provider. Usage limits never switch you to an API key automatically.',
  account: 'ChatGPT account',
  cancel: 'Cancel sign-in',
  enableUsage: 'Enable plan usage',
  reauthorize: 'Sign in again',
  addAccount: 'Add account or workspace',
  disconnect: 'Disconnect',
  manageUsage: 'Manage usage',
  retry: 'Retry plan usage',
  model: 'Polishing model',
  selectModel: 'Select an available model',
  modelUnavailable: '{{model}} is no longer available. Select another model.',
  noModels: 'No displayable models are available for this account.',
  refreshModels: 'Refresh models',
  test: 'Test connection',
  testHint:
    'Listing models does not run inference. Testing uses a small amount of your ChatGPT plan.',
  testSuccess: 'Connection test succeeded',
  remoteUnconfirmed:
    'Signed out locally, but remote revocation was not confirmed. Disconnect this app in ChatGPT settings.',
  welcome: 'You’re using your ChatGPT plan',
  gotIt: 'Got it',
  failed: 'The request did not complete. Check your connection or sign in again.',
  states: {
    signed_out: 'Not connected to ChatGPT',
    authorizing: 'Complete authorization in your system browser',
    connected: 'Using ChatGPT plan',
    usage_disabled: 'Signed in; plan usage is not enabled',
    usage_limited: 'Plan or app usage limit reached',
    reauthorization_required: 'Sign in to ChatGPT again',
  },
  errors: {
    sign_in_required: 'Sign in to ChatGPT again.',
    reauthorization_required: 'Sign in to ChatGPT again.',
    session_changed: 'Sign in to ChatGPT again.',
    refresh_interrupted: 'Sign in to ChatGPT again.',
    invalid_grant: 'Sign in to ChatGPT again.',
    invalid_refresh_token: 'Sign in to ChatGPT again.',
    token_expired: 'Sign in to ChatGPT again.',
    refresh_token_expired: 'Sign in to ChatGPT again.',
    refresh_token_invalidated: 'Sign in to ChatGPT again.',
    refresh_token_reused: 'Sign in to ChatGPT again.',
    unauthorized: 'Sign in to ChatGPT again.',
    subscription_sharing_invalid_user: 'Sign in to ChatGPT again.',
    usage_disabled: 'Allow OpenLess to use your ChatGPT plan.',
    access_denied: 'Allow OpenLess to use your ChatGPT plan.',
    chatpass_v2_scope_not_authorized: 'Allow OpenLess to use your ChatGPT plan.',
    chatpass_v2_invalid_authorization_context: 'Allow OpenLess to use your ChatGPT plan.',
    subscription_sharing_usage_limit_exceeded:
      'Your plan or this app has reached its limit. Open Manage usage.',
    rate_limited: 'Your plan or this app has reached its limit. Open Manage usage.',
    subscription_sharing_user_not_eligible:
      'This account, workspace, or region is not eligible for plan usage.',
    forbidden: 'This account, workspace, or region is not eligible for plan usage.',
    network_failed:
      'The service is temporarily unavailable. Your account information has been kept. Try again later.',
    unavailable:
      'The service is temporarily unavailable. Your account information has been kept. Try again later.',
    subscription_sharing_usage_unavailable:
      'The service is temporarily unavailable. Your account information has been kept. Try again later.',
    subscription_sharing_user_unavailable:
      'The service is temporarily unavailable. Your account information has been kept. Try again later.',
    id_token_invalid: 'This authorization could not be verified. Start a new sign-in.',
    id_token_missing: 'This authorization could not be verified. Start a new sign-in.',
    issuer_mismatch: 'This authorization could not be verified. Start a new sign-in.',
    client_mismatch: 'This authorization could not be verified. Start a new sign-in.',
    identity_mismatch: 'This authorization could not be verified. Start a new sign-in.',
    callback_invalid: 'This authorization could not be verified. Start a new sign-in.',
    state_mismatch: 'This authorization could not be verified. Start a new sign-in.',
    registration_incomplete: 'This authorization could not be verified. Start a new sign-in.',
    discovery_invalid: 'This authorization could not be verified. Start a new sign-in.',
    jwks_invalid: 'This authorization could not be verified. Start a new sign-in.',
    token_response_invalid: 'This authorization could not be verified. Start a new sign-in.',
    storage_failed:
      'Credentials could not be read or saved securely. Check your system credential store.',
    storage_corrupt:
      'Credentials could not be read or saved securely. Check your system credential store.',
    storage_unavailable:
      'Credentials could not be read or saved securely. Check your system credential store.',
    storage_full:
      'Credentials could not be read or saved securely. Check your system credential store.',
    cancelled: 'The operation was cancelled.',
    authorization_timeout: 'Authorization timed out. Start a new sign-in.',
    subscription_sharing_unsupported_capability:
      'This model or request capability is unavailable. Refresh and select another model.',
    subscription_sharing_route_not_supported:
      'This model or request capability is unavailable. Refresh and select another model.',
    models_invalid:
      'This model or request capability is unavailable. Refresh and select another model.',
    empty_response:
      'This model or request capability is unavailable. Refresh and select another model.',
    busy: 'Another account operation is in progress. Try again shortly.',
    browser_failed: 'Could not open the system browser. Please retry.',
    callback_unavailable: 'Could not open the system browser. Please retry.',
  },
};

export const siwcZhTW = {
  description:
    '符合條件的文字潤飾和翻譯使用你的 ChatGPT 方案或點數。ASR 仍使用原管道；額度不足時不會自動切換至 API Key。',
  account: 'ChatGPT 帳號',
  cancel: '取消登入',
  enableUsage: '授權方案用量',
  reauthorize: '重新授權',
  addAccount: '新增帳號或工作區',
  disconnect: '中斷連線',
  manageUsage: '管理用量',
  retry: '重試方案用量',
  model: '潤飾模型',
  selectModel: '選擇帳號可用模型',
  modelUnavailable: '模型 {{model}} 目前無法使用，請重新選擇。',
  noModels: '此帳號目前沒有可顯示的模型。',
  refreshModels: '重新整理模型',
  test: '測試連線',
  testHint: '取得模型清單不會執行推理；測試連線會使用少量 ChatGPT 方案用量。',
  testSuccess: '連線測試成功',
  remoteUnconfirmed: '已在本機登出，但未確認遠端撤銷。請至 ChatGPT 設定中中斷此應用程式。',
  welcome: '正在使用你的 ChatGPT 方案',
  gotIt: '知道了',
  failed: '請求未完成，請檢查連線或重新授權。',
  states: {
    signed_out: '尚未連接 ChatGPT',
    authorizing: '請在系統瀏覽器完成授權',
    connected: '正在使用 ChatGPT 方案',
    usage_disabled: '已登入，尚未授權方案用量',
    usage_limited: '已達方案或此應用程式的用量限制',
    reauthorization_required: '需要重新登入 ChatGPT',
  },
  errors: {
    sign_in_required: '請重新登入 ChatGPT。',
    reauthorization_required: '請重新登入 ChatGPT。',
    session_changed: '請重新登入 ChatGPT。',
    refresh_interrupted: '請重新登入 ChatGPT。',
    invalid_grant: '請重新登入 ChatGPT。',
    invalid_refresh_token: '請重新登入 ChatGPT。',
    token_expired: '請重新登入 ChatGPT。',
    refresh_token_expired: '請重新登入 ChatGPT。',
    refresh_token_invalidated: '請重新登入 ChatGPT。',
    refresh_token_reused: '請重新登入 ChatGPT。',
    unauthorized: '請重新登入 ChatGPT。',
    subscription_sharing_invalid_user: '請重新登入 ChatGPT。',
    usage_disabled: '請授權 OpenLess 使用 ChatGPT 方案用量。',
    access_denied: '請授權 OpenLess 使用 ChatGPT 方案用量。',
    chatpass_v2_scope_not_authorized: '請授權 OpenLess 使用 ChatGPT 方案用量。',
    chatpass_v2_invalid_authorization_context: '請授權 OpenLess 使用 ChatGPT 方案用量。',
    subscription_sharing_usage_limit_exceeded:
      '已達方案或此應用程式的用量限制，請開啟「管理用量」。',
    rate_limited: '已達方案或此應用程式的用量限制，請開啟「管理用量」。',
    subscription_sharing_user_not_eligible: '目前帳號、工作區或地區不符合方案用量要求。',
    forbidden: '目前帳號、工作區或地區不符合方案用量要求。',
    network_failed: '服務暫時無法連線，帳號資料已保留，請稍後重試。',
    unavailable: '服務暫時無法連線，帳號資料已保留，請稍後重試。',
    subscription_sharing_usage_unavailable: '服務暫時無法連線，帳號資料已保留，請稍後重試。',
    subscription_sharing_user_unavailable: '服務暫時無法連線，帳號資料已保留，請稍後重試。',
    id_token_invalid: '無法驗證本次授權，請重新登入。',
    id_token_missing: '無法驗證本次授權，請重新登入。',
    issuer_mismatch: '無法驗證本次授權，請重新登入。',
    client_mismatch: '無法驗證本次授權，請重新登入。',
    identity_mismatch: '無法驗證本次授權，請重新登入。',
    callback_invalid: '無法驗證本次授權，請重新登入。',
    state_mismatch: '無法驗證本次授權，請重新登入。',
    registration_incomplete: '無法驗證本次授權，請重新登入。',
    discovery_invalid: '無法驗證本次授權，請重新登入。',
    jwks_invalid: '無法驗證本次授權，請重新登入。',
    token_response_invalid: '無法驗證本次授權，請重新登入。',
    storage_failed: '無法安全讀取或儲存憑據，請檢查系統安全儲存。',
    storage_corrupt: '無法安全讀取或儲存憑據，請檢查系統安全儲存。',
    storage_unavailable: '無法安全讀取或儲存憑據，請檢查系統安全儲存。',
    storage_full: '無法安全讀取或儲存憑據，請檢查系統安全儲存。',
    cancelled: '操作已取消。',
    authorization_timeout: '授權已逾時，請重新登入。',
    subscription_sharing_unsupported_capability: '此模型或請求能力不可用，請重新整理並選擇模型。',
    subscription_sharing_route_not_supported: '此模型或請求能力不可用，請重新整理並選擇模型。',
    models_invalid: '此模型或請求能力不可用，請重新整理並選擇模型。',
    empty_response: '此模型或請求能力不可用，請重新整理並選擇模型。',
    busy: '另一個帳號操作正在進行，請稍後重試。',
    browser_failed: '無法開啟系統瀏覽器，請重試。',
    callback_unavailable: '無法開啟系統瀏覽器，請重試。',
  },
};

export const siwcJa = {
  description:
    '対象の文章校正と翻訳には ChatGPT プランまたはクレジットを使用します。音声認識は従来のプロバイダーを使い、上限到達時に API キーへ自動切り替えしません。',
  account: 'ChatGPT アカウント',
  cancel: 'ログインをキャンセル',
  enableUsage: 'プラン利用を許可',
  reauthorize: '再ログイン',
  addAccount: 'アカウント・ワークスペースを追加',
  disconnect: '接続を解除',
  manageUsage: '使用量を管理',
  retry: 'プラン利用を再試行',
  model: '校正モデル',
  selectModel: '利用可能なモデルを選択',
  modelUnavailable: '{{model}} は現在利用できません。モデルを選び直してください。',
  noModels: 'このアカウントには表示可能なモデルがありません。',
  refreshModels: 'モデルを更新',
  test: '接続をテスト',
  testHint: 'モデル一覧の取得では推論を実行しません。接続テストはプランの使用量を少量消費します。',
  testSuccess: '接続テストに成功しました',
  remoteUnconfirmed:
    '端末ではログアウトしましたが、サーバー側の失効を確認できません。ChatGPT 設定でこのアプリの接続を解除してください。',
  welcome: 'ChatGPT プランを使用しています',
  gotIt: '了解',
  failed: 'リクエストを完了できませんでした。接続を確認するか再ログインしてください。',
  states: {
    signed_out: 'ChatGPT に未接続',
    authorizing: 'システムブラウザーで認証を完了してください',
    connected: 'ChatGPT プランを使用中',
    usage_disabled: 'ログイン済み・プラン利用は未許可',
    usage_limited: 'プランまたはアプリの利用上限に達しました',
    reauthorization_required: 'ChatGPT に再ログインしてください',
  },
  errors: {
    sign_in_required: 'ChatGPT に再ログインしてください。',
    reauthorization_required: 'ChatGPT に再ログインしてください。',
    session_changed: 'ChatGPT に再ログインしてください。',
    refresh_interrupted: 'ChatGPT に再ログインしてください。',
    invalid_grant: 'ChatGPT に再ログインしてください。',
    invalid_refresh_token: 'ChatGPT に再ログインしてください。',
    token_expired: 'ChatGPT に再ログインしてください。',
    refresh_token_expired: 'ChatGPT に再ログインしてください。',
    refresh_token_invalidated: 'ChatGPT に再ログインしてください。',
    refresh_token_reused: 'ChatGPT に再ログインしてください。',
    unauthorized: 'ChatGPT に再ログインしてください。',
    subscription_sharing_invalid_user: 'ChatGPT に再ログインしてください。',
    usage_disabled: 'OpenLess に ChatGPT プランの利用を許可してください。',
    access_denied: 'OpenLess に ChatGPT プランの利用を許可してください。',
    chatpass_v2_scope_not_authorized: 'OpenLess に ChatGPT プランの利用を許可してください。',
    chatpass_v2_invalid_authorization_context:
      'OpenLess に ChatGPT プランの利用を許可してください。',
    subscription_sharing_usage_limit_exceeded:
      'プランまたはアプリの上限に達しました。「使用量を管理」を開いてください。',
    rate_limited: 'プランまたはアプリの上限に達しました。「使用量を管理」を開いてください。',
    subscription_sharing_user_not_eligible:
      'このアカウント、ワークスペース、地域ではプランを利用できません。',
    forbidden: 'このアカウント、ワークスペース、地域ではプランを利用できません。',
    network_failed:
      'サービスに接続できません。アカウント情報は保持されています。後で再試行してください。',
    unavailable:
      'サービスに接続できません。アカウント情報は保持されています。後で再試行してください。',
    subscription_sharing_usage_unavailable:
      'サービスに接続できません。アカウント情報は保持されています。後で再試行してください。',
    subscription_sharing_user_unavailable:
      'サービスに接続できません。アカウント情報は保持されています。後で再試行してください。',
    id_token_invalid: '認証を検証できません。ログインをやり直してください。',
    id_token_missing: '認証を検証できません。ログインをやり直してください。',
    issuer_mismatch: '認証を検証できません。ログインをやり直してください。',
    client_mismatch: '認証を検証できません。ログインをやり直してください。',
    identity_mismatch: '認証を検証できません。ログインをやり直してください。',
    callback_invalid: '認証を検証できません。ログインをやり直してください。',
    state_mismatch: '認証を検証できません。ログインをやり直してください。',
    registration_incomplete: '認証を検証できません。ログインをやり直してください。',
    discovery_invalid: '認証を検証できません。ログインをやり直してください。',
    jwks_invalid: '認証を検証できません。ログインをやり直してください。',
    token_response_invalid: '認証を検証できません。ログインをやり直してください。',
    storage_failed:
      '認証情報を安全に読み書きできません。システムの資格情報ストアを確認してください。',
    storage_corrupt:
      '認証情報を安全に読み書きできません。システムの資格情報ストアを確認してください。',
    storage_unavailable:
      '認証情報を安全に読み書きできません。システムの資格情報ストアを確認してください。',
    storage_full:
      '認証情報を安全に読み書きできません。システムの資格情報ストアを確認してください。',
    cancelled: '操作をキャンセルしました。',
    authorization_timeout: '認証がタイムアウトしました。再ログインしてください。',
    subscription_sharing_unsupported_capability:
      'このモデルまたは機能は利用できません。モデルを更新して選び直してください。',
    subscription_sharing_route_not_supported:
      'このモデルまたは機能は利用できません。モデルを更新して選び直してください。',
    models_invalid: 'このモデルまたは機能は利用できません。モデルを更新して選び直してください。',
    empty_response: 'このモデルまたは機能は利用できません。モデルを更新して選び直してください。',
    busy: '別のアカウント操作が進行中です。少し待って再試行してください。',
    browser_failed: 'システムブラウザーを開けません。再試行してください。',
    callback_unavailable: 'システムブラウザーを開けません。再試行してください。',
  },
};

export const siwcKo = {
  description:
    '지원되는 텍스트 다듬기와 번역에 ChatGPT 요금제 또는 크레딧을 사용합니다. 음성 인식은 기존 서비스를 사용하며 한도에 도달해도 API 키로 자동 전환하지 않습니다.',
  account: 'ChatGPT 계정',
  cancel: '로그인 취소',
  enableUsage: '요금제 사용 허용',
  reauthorize: '다시 로그인',
  addAccount: '계정 또는 워크스페이스 추가',
  disconnect: '연결 해제',
  manageUsage: '사용량 관리',
  retry: '요금제 사용 재시도',
  model: '다듬기 모델',
  selectModel: '사용 가능한 모델 선택',
  modelUnavailable: '{{model}} 모델을 사용할 수 없습니다. 다른 모델을 선택하세요.',
  noModels: '이 계정에서 표시할 수 있는 모델이 없습니다.',
  refreshModels: '모델 새로고침',
  test: '연결 테스트',
  testHint:
    '모델 목록 조회는 추론을 실행하지 않습니다. 연결 테스트는 요금제 사용량을 소량 소모합니다.',
  testSuccess: '연결 테스트 성공',
  remoteUnconfirmed:
    '이 기기에서는 로그아웃했지만 원격 취소는 확인되지 않았습니다. ChatGPT 설정에서 이 앱의 연결을 해제하세요.',
  welcome: 'ChatGPT 요금제를 사용하고 있습니다',
  gotIt: '확인',
  failed: '요청이 완료되지 않았습니다. 연결을 확인하거나 다시 로그인하세요.',
  states: {
    signed_out: 'ChatGPT에 연결되지 않음',
    authorizing: '시스템 브라우저에서 인증을 완료하세요',
    connected: 'ChatGPT 요금제 사용 중',
    usage_disabled: '로그인됨 · 요금제 사용 허용 필요',
    usage_limited: '요금제 또는 앱의 사용 한도에 도달함',
    reauthorization_required: 'ChatGPT에 다시 로그인하세요',
  },
  errors: {
    sign_in_required: 'ChatGPT에 다시 로그인하세요.',
    reauthorization_required: 'ChatGPT에 다시 로그인하세요.',
    session_changed: 'ChatGPT에 다시 로그인하세요.',
    refresh_interrupted: 'ChatGPT에 다시 로그인하세요.',
    invalid_grant: 'ChatGPT에 다시 로그인하세요.',
    invalid_refresh_token: 'ChatGPT에 다시 로그인하세요.',
    token_expired: 'ChatGPT에 다시 로그인하세요.',
    refresh_token_expired: 'ChatGPT에 다시 로그인하세요.',
    refresh_token_invalidated: 'ChatGPT에 다시 로그인하세요.',
    refresh_token_reused: 'ChatGPT에 다시 로그인하세요.',
    unauthorized: 'ChatGPT에 다시 로그인하세요.',
    subscription_sharing_invalid_user: 'ChatGPT에 다시 로그인하세요.',
    usage_disabled: 'OpenLess의 ChatGPT 요금제 사용을 허용하세요.',
    access_denied: 'OpenLess의 ChatGPT 요금제 사용을 허용하세요.',
    chatpass_v2_scope_not_authorized: 'OpenLess의 ChatGPT 요금제 사용을 허용하세요.',
    chatpass_v2_invalid_authorization_context: 'OpenLess의 ChatGPT 요금제 사용을 허용하세요.',
    subscription_sharing_usage_limit_exceeded:
      '요금제 또는 앱의 사용 한도에 도달했습니다. 사용량 관리를 여세요.',
    rate_limited: '요금제 또는 앱의 사용 한도에 도달했습니다. 사용량 관리를 여세요.',
    subscription_sharing_user_not_eligible:
      '이 계정, 워크스페이스 또는 지역은 요금제 사용 대상이 아닙니다.',
    forbidden: '이 계정, 워크스페이스 또는 지역은 요금제 사용 대상이 아닙니다.',
    network_failed: '서비스에 연결할 수 없습니다. 계정 정보는 유지됩니다. 나중에 다시 시도하세요.',
    unavailable: '서비스에 연결할 수 없습니다. 계정 정보는 유지됩니다. 나중에 다시 시도하세요.',
    subscription_sharing_usage_unavailable:
      '서비스에 연결할 수 없습니다. 계정 정보는 유지됩니다. 나중에 다시 시도하세요.',
    subscription_sharing_user_unavailable:
      '서비스에 연결할 수 없습니다. 계정 정보는 유지됩니다. 나중에 다시 시도하세요.',
    id_token_invalid: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    id_token_missing: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    issuer_mismatch: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    client_mismatch: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    identity_mismatch: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    callback_invalid: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    state_mismatch: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    registration_incomplete: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    discovery_invalid: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    jwks_invalid: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    token_response_invalid: '인증을 확인할 수 없습니다. 다시 로그인하세요.',
    storage_failed:
      '인증 정보를 안전하게 읽거나 저장할 수 없습니다. 시스템 자격 증명 저장소를 확인하세요.',
    storage_corrupt:
      '인증 정보를 안전하게 읽거나 저장할 수 없습니다. 시스템 자격 증명 저장소를 확인하세요.',
    storage_unavailable:
      '인증 정보를 안전하게 읽거나 저장할 수 없습니다. 시스템 자격 증명 저장소를 확인하세요.',
    storage_full:
      '인증 정보를 안전하게 읽거나 저장할 수 없습니다. 시스템 자격 증명 저장소를 확인하세요.',
    cancelled: '작업이 취소되었습니다.',
    authorization_timeout: '인증 시간이 초과되었습니다. 다시 로그인하세요.',
    subscription_sharing_unsupported_capability:
      '이 모델 또는 기능을 사용할 수 없습니다. 모델을 새로고침하고 다시 선택하세요.',
    subscription_sharing_route_not_supported:
      '이 모델 또는 기능을 사용할 수 없습니다. 모델을 새로고침하고 다시 선택하세요.',
    models_invalid: '이 모델 또는 기능을 사용할 수 없습니다. 모델을 새로고침하고 다시 선택하세요.',
    empty_response: '이 모델 또는 기능을 사용할 수 없습니다. 모델을 새로고침하고 다시 선택하세요.',
    busy: '다른 계정 작업이 진행 중입니다. 잠시 후 다시 시도하세요.',
    browser_failed: '시스템 브라우저를 열 수 없습니다. 다시 시도하세요.',
    callback_unavailable: '시스템 브라우저를 열 수 없습니다. 다시 시도하세요.',
  },
};

export const siwcDe = {
  description:
    'Geeignete Textüberarbeitung und Übersetzung nutzen Ihren ChatGPT-Tarif oder Ihr Guthaben. Die Spracherkennung bleibt beim bisherigen Anbieter. Bei Nutzungslimits erfolgt kein automatischer Wechsel zu einem API-Schlüssel.',
  account: 'ChatGPT-Konto',
  cancel: 'Anmeldung abbrechen',
  enableUsage: 'Tarifnutzung erlauben',
  reauthorize: 'Erneut anmelden',
  addAccount: 'Konto oder Arbeitsbereich hinzufügen',
  disconnect: 'Verbindung trennen',
  manageUsage: 'Nutzung verwalten',
  retry: 'Tarifnutzung erneut versuchen',
  model: 'Überarbeitungsmodell',
  selectModel: 'Verfügbares Modell auswählen',
  modelUnavailable: '{{model}} ist nicht mehr verfügbar. Wählen Sie ein anderes Modell.',
  noModels: 'Für dieses Konto sind keine anzeigbaren Modelle verfügbar.',
  refreshModels: 'Modelle aktualisieren',
  test: 'Verbindung testen',
  testHint:
    'Die Modellabfrage führt keine Inferenz aus. Der Verbindungstest verbraucht einen kleinen Teil Ihres ChatGPT-Kontingents.',
  testSuccess: 'Verbindungstest erfolgreich',
  remoteUnconfirmed:
    'Lokal abgemeldet, aber der Widerruf auf dem Server wurde nicht bestätigt. Trennen Sie die App in den ChatGPT-Einstellungen.',
  welcome: 'Sie nutzen Ihren ChatGPT-Tarif',
  gotIt: 'Verstanden',
  failed:
    'Die Anfrage wurde nicht abgeschlossen. Prüfen Sie die Verbindung oder melden Sie sich erneut an.',
  states: {
    signed_out: 'Nicht mit ChatGPT verbunden',
    authorizing: 'Autorisierung im Systembrowser abschließen',
    connected: 'ChatGPT-Tarif wird verwendet',
    usage_disabled: 'Angemeldet; Tarifnutzung noch nicht erlaubt',
    usage_limited: 'Nutzungslimit des Tarifs oder der App erreicht',
    reauthorization_required: 'Erneute ChatGPT-Anmeldung erforderlich',
  },
  errors: {
    sign_in_required: 'Melden Sie sich erneut bei ChatGPT an.',
    reauthorization_required: 'Melden Sie sich erneut bei ChatGPT an.',
    session_changed: 'Melden Sie sich erneut bei ChatGPT an.',
    refresh_interrupted: 'Melden Sie sich erneut bei ChatGPT an.',
    invalid_grant: 'Melden Sie sich erneut bei ChatGPT an.',
    invalid_refresh_token: 'Melden Sie sich erneut bei ChatGPT an.',
    token_expired: 'Melden Sie sich erneut bei ChatGPT an.',
    refresh_token_expired: 'Melden Sie sich erneut bei ChatGPT an.',
    refresh_token_invalidated: 'Melden Sie sich erneut bei ChatGPT an.',
    refresh_token_reused: 'Melden Sie sich erneut bei ChatGPT an.',
    unauthorized: 'Melden Sie sich erneut bei ChatGPT an.',
    subscription_sharing_invalid_user: 'Melden Sie sich erneut bei ChatGPT an.',
    usage_disabled: 'Erlauben Sie OpenLess die Nutzung Ihres ChatGPT-Tarifs.',
    access_denied: 'Erlauben Sie OpenLess die Nutzung Ihres ChatGPT-Tarifs.',
    chatpass_v2_scope_not_authorized: 'Erlauben Sie OpenLess die Nutzung Ihres ChatGPT-Tarifs.',
    chatpass_v2_invalid_authorization_context:
      'Erlauben Sie OpenLess die Nutzung Ihres ChatGPT-Tarifs.',
    subscription_sharing_usage_limit_exceeded:
      'Das Limit Ihres Tarifs oder dieser App wurde erreicht. Öffnen Sie „Nutzung verwalten“.',
    rate_limited:
      'Das Limit Ihres Tarifs oder dieser App wurde erreicht. Öffnen Sie „Nutzung verwalten“.',
    subscription_sharing_user_not_eligible:
      'Dieses Konto, dieser Arbeitsbereich oder diese Region ist nicht zur Tarifnutzung berechtigt.',
    forbidden:
      'Dieses Konto, dieser Arbeitsbereich oder diese Region ist nicht zur Tarifnutzung berechtigt.',
    network_failed:
      'Der Dienst ist vorübergehend nicht erreichbar. Ihre Kontodaten bleiben erhalten. Versuchen Sie es später erneut.',
    unavailable:
      'Der Dienst ist vorübergehend nicht erreichbar. Ihre Kontodaten bleiben erhalten. Versuchen Sie es später erneut.',
    subscription_sharing_usage_unavailable:
      'Der Dienst ist vorübergehend nicht erreichbar. Ihre Kontodaten bleiben erhalten. Versuchen Sie es später erneut.',
    subscription_sharing_user_unavailable:
      'Der Dienst ist vorübergehend nicht erreichbar. Ihre Kontodaten bleiben erhalten. Versuchen Sie es später erneut.',
    id_token_invalid:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    id_token_missing:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    issuer_mismatch:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    client_mismatch:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    identity_mismatch:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    callback_invalid:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    state_mismatch:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    registration_incomplete:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    discovery_invalid:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    jwks_invalid:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    token_response_invalid:
      'Die Autorisierung konnte nicht überprüft werden. Starten Sie eine neue Anmeldung.',
    storage_failed:
      'Anmeldedaten konnten nicht sicher gelesen oder gespeichert werden. Prüfen Sie den Systemspeicher für Zugangsdaten.',
    storage_corrupt:
      'Anmeldedaten konnten nicht sicher gelesen oder gespeichert werden. Prüfen Sie den Systemspeicher für Zugangsdaten.',
    storage_unavailable:
      'Anmeldedaten konnten nicht sicher gelesen oder gespeichert werden. Prüfen Sie den Systemspeicher für Zugangsdaten.',
    storage_full:
      'Anmeldedaten konnten nicht sicher gelesen oder gespeichert werden. Prüfen Sie den Systemspeicher für Zugangsdaten.',
    cancelled: 'Der Vorgang wurde abgebrochen.',
    authorization_timeout: 'Die Autorisierung ist abgelaufen. Melden Sie sich erneut an.',
    subscription_sharing_unsupported_capability:
      'Dieses Modell oder diese Funktion ist nicht verfügbar. Aktualisieren Sie die Modellauswahl.',
    subscription_sharing_route_not_supported:
      'Dieses Modell oder diese Funktion ist nicht verfügbar. Aktualisieren Sie die Modellauswahl.',
    models_invalid:
      'Dieses Modell oder diese Funktion ist nicht verfügbar. Aktualisieren Sie die Modellauswahl.',
    empty_response:
      'Dieses Modell oder diese Funktion ist nicht verfügbar. Aktualisieren Sie die Modellauswahl.',
    busy: 'Ein anderer Kontovorgang läuft. Versuchen Sie es gleich erneut.',
    browser_failed: 'Der Systembrowser konnte nicht geöffnet werden. Versuchen Sie es erneut.',
    callback_unavailable:
      'Der Systembrowser konnte nicht geöffnet werden. Versuchen Sie es erneut.',
  },
};

export const siwcEs = {
  description:
    'Las tareas compatibles de revisión y traducción usan tu plan o créditos de ChatGPT. El reconocimiento de voz conserva su proveedor. Si alcanzas un límite, no se cambia automáticamente a una clave API.',
  account: 'Cuenta de ChatGPT',
  cancel: 'Cancelar inicio de sesión',
  enableUsage: 'Autorizar uso del plan',
  reauthorize: 'Volver a iniciar sesión',
  addAccount: 'Añadir cuenta o espacio de trabajo',
  disconnect: 'Desconectar',
  manageUsage: 'Gestionar uso',
  retry: 'Reintentar uso del plan',
  model: 'Modelo de revisión',
  selectModel: 'Selecciona un modelo disponible',
  modelUnavailable: '{{model}} ya no está disponible. Selecciona otro modelo.',
  noModels: 'Esta cuenta no tiene modelos disponibles para mostrar.',
  refreshModels: 'Actualizar modelos',
  test: 'Probar conexión',
  testHint:
    'Consultar modelos no ejecuta inferencia. La prueba consume una pequeña parte del uso de tu plan de ChatGPT.',
  testSuccess: 'Prueba de conexión correcta',
  remoteUnconfirmed:
    'Sesión cerrada localmente, pero no se confirmó la revocación remota. Desconecta la aplicación en los ajustes de ChatGPT.',
  welcome: 'Estás usando tu plan de ChatGPT',
  gotIt: 'Entendido',
  failed: 'La solicitud no se completó. Comprueba la conexión o vuelve a iniciar sesión.',
  states: {
    signed_out: 'Sin conexión con ChatGPT',
    authorizing: 'Completa la autorización en el navegador del sistema',
    connected: 'Usando el plan de ChatGPT',
    usage_disabled: 'Sesión iniciada; falta autorizar el uso del plan',
    usage_limited: 'Límite del plan o de la aplicación alcanzado',
    reauthorization_required: 'Vuelve a iniciar sesión en ChatGPT',
  },
  errors: {
    sign_in_required: 'Vuelve a iniciar sesión en ChatGPT.',
    reauthorization_required: 'Vuelve a iniciar sesión en ChatGPT.',
    session_changed: 'Vuelve a iniciar sesión en ChatGPT.',
    refresh_interrupted: 'Vuelve a iniciar sesión en ChatGPT.',
    invalid_grant: 'Vuelve a iniciar sesión en ChatGPT.',
    invalid_refresh_token: 'Vuelve a iniciar sesión en ChatGPT.',
    token_expired: 'Vuelve a iniciar sesión en ChatGPT.',
    refresh_token_expired: 'Vuelve a iniciar sesión en ChatGPT.',
    refresh_token_invalidated: 'Vuelve a iniciar sesión en ChatGPT.',
    refresh_token_reused: 'Vuelve a iniciar sesión en ChatGPT.',
    unauthorized: 'Vuelve a iniciar sesión en ChatGPT.',
    subscription_sharing_invalid_user: 'Vuelve a iniciar sesión en ChatGPT.',
    usage_disabled: 'Autoriza a OpenLess a usar tu plan de ChatGPT.',
    access_denied: 'Autoriza a OpenLess a usar tu plan de ChatGPT.',
    chatpass_v2_scope_not_authorized: 'Autoriza a OpenLess a usar tu plan de ChatGPT.',
    chatpass_v2_invalid_authorization_context: 'Autoriza a OpenLess a usar tu plan de ChatGPT.',
    subscription_sharing_usage_limit_exceeded:
      'Se alcanzó el límite de tu plan o de esta aplicación. Abre Gestionar uso.',
    rate_limited: 'Se alcanzó el límite de tu plan o de esta aplicación. Abre Gestionar uso.',
    subscription_sharing_user_not_eligible:
      'Esta cuenta, espacio de trabajo o región no cumple los requisitos de uso del plan.',
    forbidden: 'Esta cuenta, espacio de trabajo o región no cumple los requisitos de uso del plan.',
    network_failed:
      'El servicio no está disponible temporalmente. Se conservaron los datos de tu cuenta. Inténtalo más tarde.',
    unavailable:
      'El servicio no está disponible temporalmente. Se conservaron los datos de tu cuenta. Inténtalo más tarde.',
    subscription_sharing_usage_unavailable:
      'El servicio no está disponible temporalmente. Se conservaron los datos de tu cuenta. Inténtalo más tarde.',
    subscription_sharing_user_unavailable:
      'El servicio no está disponible temporalmente. Se conservaron los datos de tu cuenta. Inténtalo más tarde.',
    id_token_invalid: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    id_token_missing: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    issuer_mismatch: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    client_mismatch: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    identity_mismatch: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    callback_invalid: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    state_mismatch: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    registration_incomplete: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    discovery_invalid: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    jwks_invalid: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    token_response_invalid: 'No se pudo verificar esta autorización. Inicia una nueva sesión.',
    storage_failed:
      'No se pudieron leer o guardar las credenciales de forma segura. Revisa el almacén de credenciales del sistema.',
    storage_corrupt:
      'No se pudieron leer o guardar las credenciales de forma segura. Revisa el almacén de credenciales del sistema.',
    storage_unavailable:
      'No se pudieron leer o guardar las credenciales de forma segura. Revisa el almacén de credenciales del sistema.',
    storage_full:
      'No se pudieron leer o guardar las credenciales de forma segura. Revisa el almacén de credenciales del sistema.',
    cancelled: 'Operación cancelada.',
    authorization_timeout: 'La autorización ha caducado. Inicia sesión de nuevo.',
    subscription_sharing_unsupported_capability:
      'Este modelo o función no está disponible. Actualiza los modelos y selecciona otro.',
    subscription_sharing_route_not_supported:
      'Este modelo o función no está disponible. Actualiza los modelos y selecciona otro.',
    models_invalid:
      'Este modelo o función no está disponible. Actualiza los modelos y selecciona otro.',
    empty_response:
      'Este modelo o función no está disponible. Actualiza los modelos y selecciona otro.',
    busy: 'Hay otra operación de cuenta en curso. Inténtalo en unos instantes.',
    browser_failed: 'No se pudo abrir el navegador del sistema. Inténtalo de nuevo.',
    callback_unavailable: 'No se pudo abrir el navegador del sistema. Inténtalo de nuevo.',
  },
};

export const siwcFr = {
  description:
    'Les tâches compatibles de révision et de traduction utilisent votre forfait ou vos crédits ChatGPT. La reconnaissance vocale conserve son fournisseur. Aucun basculement automatique vers une clé API en cas de limite.',
  account: 'Compte ChatGPT',
  cancel: 'Annuler la connexion',
  enableUsage: 'Autoriser l’utilisation du forfait',
  reauthorize: 'Se reconnecter',
  addAccount: 'Ajouter un compte ou un espace de travail',
  disconnect: 'Déconnecter',
  manageUsage: 'Gérer l’utilisation',
  retry: 'Réessayer avec le forfait',
  model: 'Modèle de révision',
  selectModel: 'Choisir un modèle disponible',
  modelUnavailable: '{{model}} n’est plus disponible. Choisissez un autre modèle.',
  noModels: 'Aucun modèle affichable n’est disponible pour ce compte.',
  refreshModels: 'Actualiser les modèles',
  test: 'Tester la connexion',
  testHint:
    'La liste des modèles ne lance pas d’inférence. Le test consomme une petite partie de votre forfait ChatGPT.',
  testSuccess: 'Test de connexion réussi',
  remoteUnconfirmed:
    'Déconnexion locale effectuée, mais la révocation à distance n’a pas été confirmée. Déconnectez l’application dans les paramètres ChatGPT.',
  welcome: 'Vous utilisez votre forfait ChatGPT',
  gotIt: 'Compris',
  failed: 'La requête n’a pas abouti. Vérifiez la connexion ou reconnectez-vous.',
  states: {
    signed_out: 'Non connecté à ChatGPT',
    authorizing: 'Terminez l’autorisation dans le navigateur système',
    connected: 'Forfait ChatGPT utilisé',
    usage_disabled: 'Connecté ; utilisation du forfait non autorisée',
    usage_limited: 'Limite du forfait ou de l’application atteinte',
    reauthorization_required: 'Reconnectez-vous à ChatGPT',
  },
  errors: {
    sign_in_required: 'Reconnectez-vous à ChatGPT.',
    reauthorization_required: 'Reconnectez-vous à ChatGPT.',
    session_changed: 'Reconnectez-vous à ChatGPT.',
    refresh_interrupted: 'Reconnectez-vous à ChatGPT.',
    invalid_grant: 'Reconnectez-vous à ChatGPT.',
    invalid_refresh_token: 'Reconnectez-vous à ChatGPT.',
    token_expired: 'Reconnectez-vous à ChatGPT.',
    refresh_token_expired: 'Reconnectez-vous à ChatGPT.',
    refresh_token_invalidated: 'Reconnectez-vous à ChatGPT.',
    refresh_token_reused: 'Reconnectez-vous à ChatGPT.',
    unauthorized: 'Reconnectez-vous à ChatGPT.',
    subscription_sharing_invalid_user: 'Reconnectez-vous à ChatGPT.',
    usage_disabled: 'Autorisez OpenLess à utiliser votre forfait ChatGPT.',
    access_denied: 'Autorisez OpenLess à utiliser votre forfait ChatGPT.',
    chatpass_v2_scope_not_authorized: 'Autorisez OpenLess à utiliser votre forfait ChatGPT.',
    chatpass_v2_invalid_authorization_context:
      'Autorisez OpenLess à utiliser votre forfait ChatGPT.',
    subscription_sharing_usage_limit_exceeded:
      'La limite de votre forfait ou de cette application est atteinte. Ouvrez Gérer l’utilisation.',
    rate_limited:
      'La limite de votre forfait ou de cette application est atteinte. Ouvrez Gérer l’utilisation.',
    subscription_sharing_user_not_eligible:
      'Ce compte, cet espace de travail ou cette région n’est pas éligible à l’utilisation du forfait.',
    forbidden:
      'Ce compte, cet espace de travail ou cette région n’est pas éligible à l’utilisation du forfait.',
    network_failed:
      'Le service est temporairement indisponible. Les données du compte sont conservées. Réessayez plus tard.',
    unavailable:
      'Le service est temporairement indisponible. Les données du compte sont conservées. Réessayez plus tard.',
    subscription_sharing_usage_unavailable:
      'Le service est temporairement indisponible. Les données du compte sont conservées. Réessayez plus tard.',
    subscription_sharing_user_unavailable:
      'Le service est temporairement indisponible. Les données du compte sont conservées. Réessayez plus tard.',
    id_token_invalid: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    id_token_missing: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    issuer_mismatch: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    client_mismatch: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    identity_mismatch: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    callback_invalid: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    state_mismatch: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    registration_incomplete:
      'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    discovery_invalid: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    jwks_invalid: 'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    token_response_invalid:
      'Cette autorisation n’a pas pu être vérifiée. Recommencez la connexion.',
    storage_failed:
      'Impossible de lire ou d’enregistrer les identifiants en toute sécurité. Vérifiez le coffre d’identifiants du système.',
    storage_corrupt:
      'Impossible de lire ou d’enregistrer les identifiants en toute sécurité. Vérifiez le coffre d’identifiants du système.',
    storage_unavailable:
      'Impossible de lire ou d’enregistrer les identifiants en toute sécurité. Vérifiez le coffre d’identifiants du système.',
    storage_full:
      'Impossible de lire ou d’enregistrer les identifiants en toute sécurité. Vérifiez le coffre d’identifiants du système.',
    cancelled: 'Opération annulée.',
    authorization_timeout: 'L’autorisation a expiré. Reconnectez-vous.',
    subscription_sharing_unsupported_capability:
      'Ce modèle ou cette fonctionnalité est indisponible. Actualisez les modèles et choisissez-en un autre.',
    subscription_sharing_route_not_supported:
      'Ce modèle ou cette fonctionnalité est indisponible. Actualisez les modèles et choisissez-en un autre.',
    models_invalid:
      'Ce modèle ou cette fonctionnalité est indisponible. Actualisez les modèles et choisissez-en un autre.',
    empty_response:
      'Ce modèle ou cette fonctionnalité est indisponible. Actualisez les modèles et choisissez-en un autre.',
    busy: 'Une autre opération sur le compte est en cours. Réessayez dans un instant.',
    browser_failed: 'Impossible d’ouvrir le navigateur système. Réessayez.',
    callback_unavailable: 'Impossible d’ouvrir le navigateur système. Réessayez.',
  },
};
