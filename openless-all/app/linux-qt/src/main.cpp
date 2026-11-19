// OpenLess Qt6 宿主
//
// 分工：
//   - Rust 侧（openless-linux-web --headless）跑完整后端 + HTTP 命令桥
//     （`/api/invoke`、`/api/events`、静态资源），命令面与 GTK/Tauri 宿主同一套。
//   - 本程序只负责窗口层：主窗口、录音胶囊、问答面板、托盘。
//
// 为什么用 Qt：QtWebEngine 是 Chromium，渲染与 Windows 的 WebView2 同族；
// 同时 Qt 走**原生 Wayland**，能用 LayerShellQt 做贴底/不给键盘/透明的浮层
// （Tauri v3 的 CEF 在 Linux 上强制 X11，这两件事都做不到）。
#include <QApplication>
#include <QFile>
#include <sys/stat.h>
#include <QDir>
#include <QThread>
#include <QLocalServer>
#include <QLocalSocket>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QCommandLineParser>
#include <QElapsedTimer>
#include <QVBoxLayout>
#include <QCloseEvent>
#include <cstdlib>
#include <unistd.h>
#include <csignal>
#include <fcntl.h>
#include <sys/file.h>
#include <sys/prctl.h>
#include <QDesktopServices>
#include <QWebEngineUrlScheme>
#include <QWebEngineUrlSchemeHandler>
#include <QWebEngineUrlRequestJob>
#include <QGuiApplication>
#include <QClipboard>
#include <QBuffer>
#include <QSet>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProcess>
#include <QProcessEnvironment>
#include <QFileInfo>
#include <QScreen>
#include <QTimer>
#include <QUrl>
#include <QWebEnginePage>
#include <QWebEngineProfile>
#include <QWebEngineScript>
#include <QWebEngineScriptCollection>
#include <QWebEngineSettings>
#include <QWebEngineView>
#include <QWindow>
#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <LayerShellQt/Shell>
#include <LayerShellQt/Window>

namespace {

/// 把页面控制台输出到宿主日志，白屏时能立刻看出是 JS 错还是加载失败。
class LoggingPage : public QWebEnginePage {
public:
    using QWebEnginePage::QWebEnginePage;

protected:
    /// 剪贴板通道：页面执行 `alert('openless-copy:<text>')`，这里直接写系统
    /// 剪贴板并吞掉弹框。比自定义协议/fetch 都可靠（没有 CORS 与激活限制）。
    void javaScriptAlert(const QUrl &securityOrigin, const QString &message) override {
        Q_UNUSED(securityOrigin);
        const QString openPrefix = QStringLiteral("openless-open:");
        if (message.startsWith(openPrefix)) {
            const QString url = message.mid(openPrefix.size()).trimmed();
            if (url.startsWith(QLatin1String("http")) || url.startsWith(QLatin1String("https"))) {
                QDesktopServices::openUrl(QUrl(url));
                qInfo("external open (alert channel): %s", qPrintable(url));
            }
            return;
        }
        const QString prefix = QStringLiteral("openless-copy:");
        if (message.startsWith(prefix)) {
            const QString text = message.mid(prefix.size());
            QGuiApplication::clipboard()->setText(text, QClipboard::Clipboard);
            qInfo("clipboard: %d chars via alert channel", static_cast<int>(text.size()));
            return;
        }
        qInfo("[alert] %s", qPrintable(message));
    }

    void javaScriptConsoleMessage(JavaScriptConsoleMessageLevel level, const QString &message,
                                 int line, const QString &source) override {
        if (level == InfoMessageLevel && !message.contains("openless", Qt::CaseInsensitive)) {
            return;
        }
        qInfo("[page %d] %s (%s:%d)", static_cast<int>(level), qPrintable(message),
              qPrintable(source), line);
    }

    /// 页面里跳到外部站点（例如「发布说明」链接）时交给系统浏览器，
    /// 不在应用内导航走 —— 否则会把整个应用界面替换成网页。
    bool acceptNavigationRequest(const QUrl &url, NavigationType type, bool isMainFrame) override {
        if (isMainFrame && type == NavigationTypeLinkClicked && isExternal(url)) {
            QDesktopServices::openUrl(url);
            return false;
        }
        return QWebEnginePage::acceptNavigationRequest(url, type, isMainFrame);
    }

public:
    /// 本地宿主（127.0.0.1 的静态资源与 /api）之外的都算外部。
    static bool isExternal(const QUrl &url) {
        const QString host = url.host();
        return !(host == QLatin1String("127.0.0.1") || host == QLatin1String("localhost") ||
                 host.isEmpty() || url.scheme() == QLatin1String("data") ||
                 url.scheme() == QLatin1String("blob"));
    }
};

/// 宿主注入：让前端知道渲染层是 Chromium，且不要画二次标题栏。
const char *kHostScript =
    "window.__openlessHost='qt';"
    // 剪贴板：QtWebEngine 在 Wayland 下经常没有 navigator.clipboard（或写入被
    // 焦点/权限挡住），History 的复制按钮就会报 "clipboard unavailable"。
    // 这里直接接管 writeText，走宿主命令（fcitx5 剪贴板，不依赖焦点）。
    "(function(){"
    "function put(text){"
    // 1) 宿主立即写系统剪贴板（Qt 是焦点客户端，写入选区可靠）。
    "alert('openless-copy:'+String(text));"
    // 2) 同时把文本交给 fcitx5 插件（进入输入法剪贴板历史）。
    //    插件的 setClipboard 在部分合成器上只能更新 fcitx 自己的缓存、
    //    拿不到真实选区所有权（实测：返回 true 但 wl-paste 内容不变），
    //    所以它**不能**是唯一的写入路径。
    "try{fetch('/api/invoke',{method:'POST',headers:{'Content-Type':'application/json'},"
    "body:JSON.stringify({id:'clip-'+Date.now(),command:'copy_text_to_clipboard',"
    "args:{text:String(text)}})});}catch(e){}"
    "return Promise.resolve();"
    "}"
    "function installDiagnostics(){"
    // 前端把很多失败吞进 UI 提示里，控制台看不到。这里把它们的文案打出来，
    // 宿主日志（journal）就能查到真正原因。
    "try{"
    "window.addEventListener('openless:sync-ui-persistence-failed',function(e){"
    "var d=e.detail;"
    "var msg=d&&(d.message||d.code||d);"
    "console.error('openless-sync-ui-failed: '+String(msg));"
    "});"
    "window.addEventListener('unhandledrejection',function(e){"
    "var r=e.reason;var msg=r&&(r.message||r.code||r);"
    "console.error('openless-unhandled-rejection: '+String(msg));"
    "});"
    "window.addEventListener('error',function(e){"
    "console.error('openless-window-error: '+String(e.message));"
    "});"
    "}catch(e){}"
    "}"
    "function installOpen(){"
    "try{"
    "window.open=function(url){"
    "if(url){alert('openless-open:'+String(url));}"
    "return null;"
    "};"
    "console.log('openless window.open shim installed');"
    "}catch(e){console.log('openless window.open shim failed: '+e);}"
    "}"
    "function install(){"
    "var clip=null;"
    "try{clip=navigator.clipboard;}catch(e){}"
    "if(!clip){clip={};Object.defineProperty(Navigator.prototype,'clipboard',"
    "{value:clip,configurable:true});}"
    "try{clip.writeText=function(text){return put(text);};"
    "console.log('openless clipboard shim installed');}catch(e){"
    "console.log('openless clipboard shim failed: '+e);}"
    "}"
    "install();"
    "installOpen();"
    "installDiagnostics();"
    "})();"
    "(function(){var d=document.documentElement;"
    "if(d){d.dataset.olEngine='chromium';}"
    "else{document.addEventListener('DOMContentLoaded',function(){"
    "document.documentElement.dataset.olEngine='chromium';});}})();";

/// `openless:` 自定义协议：页面把文本 POST 过来，宿主直接写系统剪贴板。
///
/// 为什么不走 fcitx5 插件：KWin/wayland 下后台进程（fcitx5）没有输入事件
/// 序列，**无法夺取剪贴板所有权**（插件调用返回 true 但选择区不变，实测）。
/// 而 Qt 窗口是当前焦点客户端，`QClipboard::setText` 是可靠路径。
class ClipboardSchemeHandler : public QWebEngineUrlSchemeHandler {
public:
    explicit ClipboardSchemeHandler(QObject *parent = nullptr)
        : QWebEngineUrlSchemeHandler(parent) {}

    void requestStarted(QWebEngineUrlRequestJob *job) override {
#if QT_VERSION >= QT_VERSION_CHECK(6, 6, 0)
        QIODevice *body = job->requestBody();
        const QByteArray payload = body ? body->readAll() : QByteArray();
#else
        // Qt < 6.6 没有 requestBody()：这条协议只用于「把文本交给宿主剪贴板」，
        // 老 Qt 上由页面的 navigator.clipboard shim 直接走 alert 通道，这里返回空。
        const QByteArray payload;
#endif
        const QString text = QString::fromUtf8(payload);
        QGuiApplication::clipboard()->setText(text, QClipboard::Clipboard);
        qInfo("clipboard: %d chars via host", static_cast<int>(text.size()));
        auto *buffer = new QBuffer();
        buffer->setData(QByteArrayLiteral("ok"));
        buffer->open(QIODevice::ReadOnly);
        job->reply("text/plain", buffer);
    }
};

enum class Surface { Main, Capsule, Qa, VoiceEdit };

/// 顶层容器。主窗口关闭时只隐藏，不退出进程（托盘常驻）。
class TopLevelWindow : public QWidget {
public:
    explicit TopLevelWindow(bool closeHides) : closeHides_(closeHides) {}

protected:
    void closeEvent(QCloseEvent *event) override {
        if (closeHides_) {
            hide();
            event->ignore();
            return;
        }
        QWidget::closeEvent(event);
    }

private:
    bool closeHides_;
};

/// OpenLess 的图标：优先主题名 `openless`，其次包内 hicolor 尺寸，最后兜底。
static QIcon openlessIcon() {
    QIcon icon = QIcon::fromTheme("openless");
    if (!icon.isNull()) return icon;
    for (const QString &candidate :
         {QStringLiteral("/usr/share/icons/hicolor/256x256/apps/openless.png"),
          QStringLiteral("/usr/share/icons/hicolor/128x128/apps/openless.png"),
          QStringLiteral("/usr/share/pixmaps/openless.png")}) {
        if (QFileInfo::exists(candidate)) return QIcon(candidate);
    }
    return QIcon::fromTheme("audio-input-microphone");
}

/// 找出当前用户下还活着的后端进程（`openless-linux-web`）。
///
/// 只在「已经确认没有第二个 Qt 宿主在跑」时调用：那种情况下活着的后端只可能是
/// 上次宿主崩溃留下的孤儿（父进程没了，PDEATHSIG 也就无人触发），它占着
/// `openless-web.sock`，会让新后端启动即退出 —— 表现为「点图标起不来」。
static QList<qint64> orphanBackendPids(const QString &expectedBinary) {
    QList<qint64> pids;
    const QByteArray expected = QFile::encodeName(QFileInfo(expectedBinary).fileName());
    const QDir proc(QStringLiteral("/proc"));
    for (const QString &entry : proc.entryList(QDir::Dirs | QDir::NoDotAndDotDot)) {
        bool numeric = false;
        const qint64 pid = entry.toLongLong(&numeric);
        if (!numeric || pid == ::getpid()) continue;
        const QByteArray link = QFile::encodeName(QStringLiteral("/proc/%1/exe").arg(pid));
        char buffer[4096];
        const ssize_t length = ::readlink(link.constData(), buffer, sizeof(buffer) - 1);
        if (length <= 0) continue;
        buffer[length] = '\0';
        const QByteArray exe(buffer);
        if (exe.endsWith('/' + expected)) pids.append(pid);
    }
    return pids;
}

/// 一个 webview 窗口。胶囊/问答用 layer-shell，主窗口是普通窗口。
class SurfaceWindow {
public:
    SurfaceWindow(Surface kind, const QString &baseUrl, QObject *parent);
    ~SurfaceWindow();

    QWebEngineView *view() const { return view_; }
    void load();
    void setVisible(bool visible);
    /// 抬到前台并请求激活（后续实例点图标、托盘「打开」都走这里）。
    void raise();
    void setExpanded(bool expanded);
    bool isVisible() const { return visible_; }
    Surface kind() const { return kind_; }

private:
    void applyLayerShell();

    Surface kind_;
    QString url_;
    QWebEngineView *view_ = nullptr;
    /// 浮层用普通 QWidget 作顶层窗口，webview 只是子控件。
    /// QWebEngineView 自己作顶层时 LayerShellQt 的 surface 起不来（实测不映射）。
    QWidget *container_ = nullptr;
    QWidget *topLevel() const { return container_ ? container_ : view_; }
    bool visible_ = false;
    bool expanded_ = false;
    QSize compactSize_;
    QSize expandedSize_;
};

SurfaceWindow::SurfaceWindow(Surface kind, const QString &baseUrl, QObject *parent)
    : kind_(kind), view_(nullptr) {
    view_ = new QWebEngineView();
    view_->setPage(new LoggingPage(view_));
    // 让页面能用 openless:copy 直接写系统剪贴板。
    static ClipboardSchemeHandler *clipboardHandler = nullptr;
    if (!clipboardHandler) {
        clipboardHandler = new ClipboardSchemeHandler(qApp);
    }
    QWebEngineProfile::defaultProfile()->installUrlSchemeHandler("openless",
                                                                 clipboardHandler);
    container_ = new TopLevelWindow(kind != Surface::Main ? false : true);
    {
        auto *layout = new QVBoxLayout(container_);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->setSpacing(0);
        layout->addWidget(view_);
    }

    auto *settings = view_->settings();
    settings->setAttribute(QWebEngineSettings::JavascriptEnabled, true);
    settings->setAttribute(QWebEngineSettings::LocalContentCanAccessRemoteUrls, true);

    QString windowKind;
    switch (kind) {
        case Surface::Main:
            // 主窗口**不带** `window` 参数：前端用 `!params.get('window')`
            // 判断主窗口（`isMainWindow`），带了就会跳过主窗口的初始化，
            // 包括云同步 UI 桥的安装。
            windowKind.clear();
            container_->setWindowFlags(Qt::Window);
            container_->setWindowTitle("OpenLess");
            container_->resize(1180, 820);
            compactSize_ = QSize(1180, 820);
            break;
        case Surface::Capsule:
            windowKind = "capsule";
            container_->setWindowFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint |
                                       Qt::Tool);
            container_->setAttribute(Qt::WA_TranslucentBackground, true);
            container_->setAttribute(Qt::WA_NoSystemBackground, true);
            container_->setStyleSheet("background: transparent;");
            view_->setAttribute(Qt::WA_TranslucentBackground, true);
            view_->setStyleSheet("background: transparent;");
            view_->page()->setBackgroundColor(Qt::transparent);
            compactSize_ = QSize(460, 228);
            break;
        case Surface::VoiceEdit:
            windowKind = "voice-edit";
            container_->setWindowFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint |
                                       Qt::Tool | Qt::WindowDoesNotAcceptFocus);
            container_->setAttribute(Qt::WA_ShowWithoutActivating);
            compactSize_ = QSize(680, 520);
            break;
        case Surface::Qa:
            windowKind = "qa";
            container_->setWindowFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint |
                                       Qt::Tool);
            container_->setAttribute(Qt::WA_TranslucentBackground, true);
            container_->setAttribute(Qt::WA_NoSystemBackground, true);
            container_->setStyleSheet("background: transparent;");
            view_->setAttribute(Qt::WA_TranslucentBackground, true);
            view_->setStyleSheet("background: transparent;");
            view_->page()->setBackgroundColor(Qt::transparent);
            compactSize_ = QSize(480, 80);
            expandedSize_ = QSize(760, 416);
            break;
    }
    container_->resize(compactSize_);
    view_->resize(compactSize_);

    QWebEngineScript script;
    script.setName("openless-host-tag");
    script.setInjectionPoint(QWebEngineScript::DocumentCreation);
    script.setRunsOnSubFrames(false);
    script.setWorldId(QWebEngineScript::MainWorld);
    script.setSourceCode(QString::fromUtf8(kHostScript));
    view_->page()->scripts().insert(script);

    // 排障：OPENLESS_QT_URL_CAPSULE / _QA / _MAIN 可以覆盖某个窗口加载的地址，
    // 用来单独验证透明/定位（例如塞一个纯色测试页）。
    const char *overrideKey = kind == Surface::Main   ? "OPENLESS_QT_URL_MAIN"
                              : kind == Surface::Qa ? "OPENLESS_QT_URL_QA"
                                                    : "OPENLESS_QT_URL_CAPSULE";
    url_ = qEnvironmentVariable(overrideKey);
    if (url_.isEmpty()) {
        url_ = windowKind.isEmpty()
                   ? QString("%1/index.html?bridge=http").arg(baseUrl)
                   : QString("%1/index.html?bridge=http&window=%2").arg(baseUrl, windowKind);
    }

    QObject::connect(view_, &QWebEngineView::loadFinished, view_, [this](bool ok) {
        qInfo("surface=%d loadFinished=%d", static_cast<int>(kind_), ok ? 1 : 0);
    });
    QObject::connect(view_->page(), &QWebEnginePage::renderProcessTerminated, view_,
                     [](QWebEnginePage::RenderProcessTerminationStatus status, int code) {
                         qInfo("renderProcessTerminated status=%d code=%d",
                               static_cast<int>(status), code);
                     });
    Q_UNUSED(parent);
}

SurfaceWindow::~SurfaceWindow() {
    delete container_;
    delete view_;
}

void SurfaceWindow::load() {
    if (kind_ != Surface::Main) {
        // layer-shell 必须在**平台窗口创建之前**安装集成：
        // `winId()` 会提前创建平台窗口（xdg-shell 已绑定），所以改用
        // `windowHandle()` —— 只创建 QWindow，真正的 surface 延迟到 show()。
        container_->winId();
        applyLayerShell();
        // LayerShellQt 的集成只在 surface 创建时生效：配置完要 hide + show
        // 让平台 surface 按 layer-shell 重建（否则会报 already has a shell
        // integration，或干脆不显示）。
        // 重建 surface 以应用 layer-shell；随后立刻按当前可见性恢复
        //（否则浮层会在启动时被这一次 show 强行显示出来）。
        container_->hide();
        container_->show();
        topLevel()->setVisible(visible_);
    }
    view_->load(QUrl(url_));
    qInfo("surface=%d url=%s", static_cast<int>(kind_), qPrintable(url_));
}

void SurfaceWindow::applyLayerShell() {
    auto *window = container_->windowHandle();
    if (!window) return;
    auto *layerWindow = LayerShellQt::Window::get(window);
    if (!layerWindow) {
        qInfo("surface=%d layer-shell 不可用（非 Wayland？）", static_cast<int>(kind_));
        return;
    }
    layerWindow->setLayer(LayerShellQt::Window::LayerOverlay);
    layerWindow->setExclusiveZone(-1);
    if (kind_ == Surface::Capsule || kind_ == Surface::VoiceEdit) {
        layerWindow->setAnchors(LayerShellQt::Window::AnchorBottom);
        layerWindow->setMargins(QMargins(0, 0, 0, 12));
        // 胶囊出现时用户正在别的应用里说话：绝不能抢焦点，
        // 否则 fcitx5 的插入目标会变成我们自己。
        layerWindow->setKeyboardInteractivity(
            LayerShellQt::Window::KeyboardInteractivityNone);
    } else {
        layerWindow->setAnchors(LayerShellQt::Window::AnchorBottom);
        layerWindow->setMargins(QMargins(0, 0, 0, 8));
        // 问答面板要收键盘输入，所以按需给焦点（不是 None）。
        layerWindow->setKeyboardInteractivity(
            LayerShellQt::Window::KeyboardInteractivityOnDemand);
    }
    qInfo("surface=%d layer-shell 已安装", static_cast<int>(kind_));
}

void SurfaceWindow::raise() {
    topLevel()->setWindowIcon(openlessIcon());
    QWidget *window = topLevel();
    window->setVisible(true);
    window->raise();
    window->activateWindow();
}

void SurfaceWindow::setVisible(bool visible) {
    visible_ = visible;
    // 不能拿缓存标志当「已可见」：主窗口关闭时走的是 closeEvent→hide()，
    // 缓存里还是 true，于是托盘点「打开」会因为 `visible_ == true` 直接返回，
    // 窗口再也不出来。以实际可见性为准。
    if (visible == topLevel()->isVisible()) {
        if (visible) {
            topLevel()->raise();
            if (kind_ != Surface::VoiceEdit) topLevel()->activateWindow();
        }
        return;
    }
    if (visible) {
        topLevel()->show();
        topLevel()->raise();
        if (kind_ != Surface::VoiceEdit) topLevel()->activateWindow();
    } else {
        topLevel()->hide();
    }
}

void SurfaceWindow::setExpanded(bool expanded) {
    if (kind_ != Surface::Qa || expanded_ == expanded) return;
    expanded_ = expanded;
    topLevel()->resize(expanded ? expandedSize_ : compactSize_);
    view_->resize(expanded ? expandedSize_ : compactSize_);
}

}  // namespace

int main(int argc, char **argv) {
    // 浮层是 layer-shell surface，QQuickWidget（QWebEngineView 内部）默认走
    // GL/RHI 时拿不到合适的 surface。软件渲染后端让 webview 走 raster，
    // 不再依赖 GL surface（代价是动画稍慢，浮层只有几十像素，可接受）。
    // 默认软件渲染：这是 layer-shell 浮层能正常映射的前提（GL/RHI surface
    // 在 layer-shell 下起不来，实测窗口不显示）。代价是动画略慢；
    // 想要 GPU 渲染可用 OPENLESS_QT_GPU=1 关掉（浮层会失效，仅排障用）。
    if (!qEnvironmentVariableIsSet("OPENLESS_QT_GPU")) {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);
    }

    // 自定义协议必须在 QApplication 之前注册。
    {
        QWebEngineUrlScheme scheme(QByteArrayLiteral("openless"));
#if QT_VERSION >= QT_VERSION_CHECK(6, 6, 0)
        scheme.setFlags(QWebEngineUrlScheme::FetchApiAllowed |
                        QWebEngineUrlScheme::LocalScheme);
#else
        scheme.setFlags(QWebEngineUrlScheme::LocalScheme);
#endif
        QWebEngineUrlScheme::registerScheme(scheme);
    }

    QApplication app(argc, argv);
    app.setApplicationName("OpenLess");
    app.setApplicationDisplayName("OpenLess");
    // Wayland 的 app_id 取自 desktopFileName（缺省时退回可执行文件名
    // `openless-qt`）；必须在 `openless.desktop` 里能查到，否则任务栏与
    // 窗口装饰只能显示通用的 Wayland 图标。
    app.setDesktopFileName(QStringLiteral("openless"));
    // X11 及不支持按 .desktop 取图标的桌面用它兜底。
    app.setWindowIcon(openlessIcon());
    app.setQuitOnLastWindowClosed(false);
    const bool minimized = app.arguments().contains(QStringLiteral("--minimized"));

    // ── 0. 单实例 ───────────────────────────────────────────────────────────
    // 托盘 / 桌面图标每点一次都会执行一次 `/usr/bin/openless`。这里必须用**原子**
    // 的方式判断谁是首实例：早先用「先 connect，连不上再 listen」有竞态 —— 两个
    // 进程在对方还没 listen 时同时进来，两个都会认为自己 是首实例，于是出现两套
    // 窗口与两个托盘图标（实测发生过）。
    //
    // 现在用 flock 文件锁：拿到锁的是首实例，拿不到的转交给它并退出。
    const QString runtimeDir =
        qEnvironmentVariable("XDG_RUNTIME_DIR", QStringLiteral("/tmp"));
    const QString lockPath = runtimeDir + QStringLiteral("/openless.lock");
    int instanceLock = ::open(QFile::encodeName(lockPath).constData(),
                              O_RDWR | O_CREAT | O_CLOEXEC, 0600);
    bool isPrimary = false;
    if (instanceLock < 0) {
        qWarning("无法创建单实例锁 %s：继续启动（可能多开）", qPrintable(lockPath));
        isPrimary = true;
    } else if (::flock(instanceLock, LOCK_EX | LOCK_NB) == 0) {
        isPrimary = true;
        // 锁文件永不删除：删掉会让后来者拿到一个新 inode，锁就失效了。
    } else {
        ::close(instanceLock);
        instanceLock = -1;
    }

    const QString instanceKey =
        QStringLiteral("openless-qt-%1").arg(static_cast<qulonglong>(::geteuid()));
    if (!isPrimary) {
        if (minimized) return 0;
        // 已有实例：让它抬起窗口，本进程退出。等一小会儿是因为对方的监听
        // socket 可能刚建好（它也要跑完 QApplication 初始化）。
        for (int attempt = 0; attempt < 30; ++attempt) {
            QLocalSocket probe;
            probe.connectToServer(instanceKey);
            if (probe.waitForConnected(200)) {
                probe.write("show\n");
                probe.flush();
                probe.waitForBytesWritten(300);
                break;
            }
            QThread::msleep(100);
        }
        fprintf(stderr, "openless-qt: another instance is running; handing over and exiting\n");
        return 0;
    }
    // 首实例：清掉上次崩溃遗留的 socket，然后自己监听。
    QLocalServer::removeServer(instanceKey);
    auto *instanceServer = new QLocalServer(&app);
    if (!instanceServer->listen(instanceKey)) {
        qWarning("单实例监听失败（%s）：继续启动，可能出现多开",
                 qPrintable(instanceServer->errorString()));
    }
    // ── 1. 拉起 Rust 后端（无头），从 stdout 里读端口 ────────────────────────
    QString baseUrl = qEnvironmentVariable("OPENLESS_HOST_URL");
    QProcess *backend = nullptr;
    if (baseUrl.isEmpty()) {
        backend = new QProcess(&app);
        backend->setProcessChannelMode(QProcess::MergedChannels);
        // 关键：后端必须无头运行。否则它自己会建 GTK 主窗口和胶囊，
        // 与 Qt 的三个窗口重叠（日志里会出现两份 capsule）。
        QProcessEnvironment backendEnv = QProcessEnvironment::systemEnvironment();
        backendEnv.insert("OPENLESS_HEADLESS", "1");
        backendEnv.insert("OPENLESS_GUI_EXECUTABLE", QCoreApplication::applicationFilePath());
        backend->setProcessEnvironment(backendEnv);
        // 默认与自身同目录（安装后是 /usr/lib/openless），这样解包试跑也能找到后端。
        QString defaultHost = QCoreApplication::applicationDirPath() + "/openless-linux-web";
        if (!QFileInfo::exists(defaultHost)) {
            defaultHost = QStringLiteral("/usr/lib/openless/openless-linux-web");
        }
        const QString hostBin = qEnvironmentVariable("OPENLESS_HOST_BIN", defaultHost);
        QStringList args;
        // 后端必须随宿主一起退出：宿主被 kill/崩溃后留下的孤儿后端会一直占着
        // `$XDG_RUNTIME_DIR/openless-web.sock`，下一个后端启动时抢不到就立刻退出，
        // 宿主于是等端口等到超时 —— 表现就是「点图标没反应 / 起不来」。
        backend->setChildProcessModifier([]() { prctl(PR_SET_PDEATHSIG, SIGTERM); });
        backend->start(hostBin, args);
        if (!backend->waitForStarted(5000)) {
            qCritical("无法启动后端 %s", qPrintable(hostBin));
            return 1;
        }
        QElapsedTimer timer;
        timer.start();
        bool retriedAfterStaleSocket = false;
        while (timer.elapsed() < 20000 && baseUrl.isEmpty()) {
            if (backend->state() != QProcess::Running && backend->state() != QProcess::Starting) {
                // 后端刚起就退出：几乎一定是单实例锁被占。
                const QString socketPath =
                    QStringLiteral("%1/openless-web.sock")
                        .arg(QString::fromLocal8Bit(qgetenv("XDG_RUNTIME_DIR")));
                bool holderAlive = false;
                {
                    QLocalSocket probe;
                    probe.connectToServer(socketPath);
                    holderAlive = probe.waitForConnected(200);
                }
                if (!holderAlive && !retriedAfterStaleSocket) {
                    // 空壳 socket：清掉再来一次。
                    qWarning("后端启动失败（socket 是上次崩溃的残留），清理后重试一次");
                    QLocalServer::removeServer(socketPath);
                    QFile::remove(socketPath);
                    retriedAfterStaleSocket = true;
                    backend->start(hostBin, args);
                    continue;
                }
                if (holderAlive && !retriedAfterStaleSocket) {
                    // 走到这里说明没有第二个 Qt 宿主（上面的单实例探测已经排除），
                    // 那占着后端的就只可能是上次崩溃留下的孤儿：回收它再重试一次。
                    const QList<qint64> holders = orphanBackendPids(hostBin);
                    for (qint64 pid : holders) {
                        qWarning("回收孤儿后端进程 %lld（上次宿主退出时没被带走）",
                                 static_cast<long long>(pid));
                        ::kill(static_cast<pid_t>(pid), SIGTERM);
                    }
                    if (!holders.isEmpty()) {
                        QThread::msleep(500);
                        QLocalServer::removeServer(socketPath);
                        QFile::remove(socketPath);
                        retriedAfterStaleSocket = true;
                        backend->start(hostBin, args);
                        continue;
                    }
                }
                qCritical("后端无法启动：另一个 OpenLess 后端占着 %s，且无法回收。"
                          "先结束它（pkill -f openless-linux-web）再启动。",
                          qPrintable(socketPath));
                return 1;
            }
            backend->waitForReadyRead(200);
            const QString output = QString::fromUtf8(backend->readAll());
            for (const QString &line : output.split('\n')) {
                const int index = line.indexOf("127.0.0.1:");
                if (index < 0) continue;
                QString rest = line.mid(index + 10);
                rest = rest.left(rest.indexOf('/'));
                if (!rest.isEmpty()) baseUrl = QString("http://127.0.0.1:%1").arg(rest);
            }
        }
        if (baseUrl.isEmpty()) {
            qCritical("后端 20 秒内没有报告端口");
            return 1;
        }
        qInfo("后端就绪：%s", qPrintable(baseUrl));
        // 后端输出继续转到宿主日志（排障用）。
        QObject::connect(backend, &QProcess::readyRead, &app, [backend]() {
            const QString text = QString::fromUtf8(backend->readAll()).trimmed();
            if (!text.isEmpty()) qInfo("[backend] %s", qPrintable(text));
        });
    }

    // 显式启用 layer-shell 集成（Qt 6.5+ 不再必需，但显式声明能避免
    // 平台集成在窗口创建顺序上被 xdg-shell 抢走）。
    LayerShellQt::Shell::useLayerShell();

    // ── 2. 三个窗口 ─────────────────────────────────────────────────────────
    SurfaceWindow mainWindow(Surface::Main, baseUrl, &app);
    SurfaceWindow capsule(Surface::Capsule, baseUrl, &app);
    SurfaceWindow qa(Surface::Qa, baseUrl, &app);
    SurfaceWindow voiceEdit(Surface::VoiceEdit, baseUrl, &app);
    voiceEdit.load();
    mainWindow.load();
    capsule.load();
    qa.load();
    mainWindow.setVisible(qEnvironmentVariableIsSet("OPENLESS_QT_SHOW_MAIN") ||
                          (!minimized && !qEnvironmentVariableIsSet("OPENLESS_QT_HIDE_MAIN")));
    if (qEnvironmentVariableIsSet("OPENLESS_QT_SHOW_CAPSULE")) capsule.setVisible(true);
    if (qEnvironmentVariableIsSet("OPENLESS_QT_SHOW_QA")) qa.setVisible(true);

    // 后续实例点图标 → 这里抬起已有窗口。事件循环在窗口建好之后才跑，
    // 所以启动期间到达的连接也会在 exec() 之后由这个 handler 收下。
    QObject::connect(instanceServer, &QLocalServer::newConnection, &app,
                     [instanceServer, &mainWindow]() {
        while (QLocalSocket *connection = instanceServer->nextPendingConnection()) {
            QObject::connect(connection, &QLocalSocket::readyRead, connection,
                             [connection, &mainWindow]() {
                                 const QByteArray request = connection->readAll();
                                 if (request.contains("show")) {
                                     fprintf(stderr,
                                             "openless-qt: show request from second instance\n");
                                     mainWindow.raise();
                                 }
                                 connection->disconnectFromServer();
                             });
            QObject::connect(connection, &QLocalSocket::disconnected, connection,
                             &QLocalSocket::deleteLater);
        }
    });

    // ── 3. 托盘 ─────────────────────────────────────────────────────────────
    // 托盘由 Rust 侧（`linux-web`）注册 StatusNotifierItem：与 macOS/Windows 共用
    // 同一套菜单（显示主窗口 / 输出风格 / 选择麦克风 / 退出）和同一份 8 语言文案。
    // 这里的窗口只负责响应它推来的 `tray:command` 事件。

    // ── 4. 事件：dictation:state 控制胶囊，qa:state 控制问答面板 ────────────
    auto *manager = new QNetworkAccessManager(&app);
    auto *reply = manager->get(QNetworkRequest(QUrl(baseUrl + "/api/events")));
    auto *buffer = new QByteArray();
    auto *capsuleHideTimer = new QTimer(&app);
    capsuleHideTimer->setSingleShot(true);
    QObject::connect(capsuleHideTimer, &QTimer::timeout, [&capsule]() {
        capsule.setVisible(false);
    });
    QObject::connect(reply, &QNetworkReply::readyRead, &app,
                     [reply, buffer, &mainWindow, &capsule, &qa, &voiceEdit, capsuleHideTimer]() {
        buffer->append(reply->readAll());
        int index;
        while ((index = buffer->indexOf("\n\n")) >= 0) {
            const QByteArray frame = buffer->left(index);
            buffer->remove(0, index + 2);
            for (const QByteArray &line : frame.split('\n')) {
                if (!line.startsWith("data: ")) continue;
                QJsonParseError error{};
                const auto document = QJsonDocument::fromJson(line.mid(6), &error);
                if (error.error != QJsonParseError::NoError || !document.isObject()) continue;
                const auto object = document.object();
                const QString name = object.value("name").toString();
                const QString payload = object.value("payload").toString();
                const auto inner = QJsonDocument::fromJson(payload.toUtf8()).object();
                if (name == "dictation:state") {
                    // DictationPhase: idle/starting/recording/transcribing/
                    // polishing/inserting/completed/cancelled/failed。
                    // 只有「进行中」的阶段才显示胶囊；completed/cancelled/failed
                    // 都是终态，必须收起（否则取消录音后胶囊一直挂在屏幕上）。
                    const QString phase = inner.value("phase").toString();
                    static const QSet<QString> activePhases = {
                        QStringLiteral("starting"), QStringLiteral("recording"),
                        QStringLiteral("transcribing"), QStringLiteral("polishing"),
                        QStringLiteral("inserting")};
                    capsule.setVisible(activePhases.contains(phase));
                } else if (name == "host:action") {
                    // core 的宿主动作：窗口显隐、打开外链、重启、系统设置。
                    const QString action = inner.value("action").toString();
                    if (action == "showQa") {
                        qa.setVisible(true);
                    } else if (action == "hideQa") {
                        qa.setVisible(false);
                    } else if (action == "showMain") {
                        mainWindow.raise();
                    } else if (action == "openUrl") {
                        QDesktopServices::openUrl(QUrl(inner.value("url").toString()));
                    } else if (action == "restart") {
                        QProcess::startDetached(QCoreApplication::applicationFilePath(), {});
                        QCoreApplication::quit();
                    }
                } else if (name == "voice-edit:show") {
                    voiceEdit.setVisible(true);
                } else if (name == "voice-edit:hide") {
                    voiceEdit.setVisible(false);
                } else if (name == "selection-polish-preview:shown") {
                    // 润色预览复用问答面板（与 Tauri 一致），面板自己拉负载。
                    qa.setVisible(true);
                } else if (name == "tray:command") {
                    // payload 是 JSON 字符串："show" / "quit"
                    if (payload.contains("quit")) {
                        QCoreApplication::quit();
                    } else if (payload.contains("show")) {
                        mainWindow.raise();
                    }
                } else if (name == "selection:state") {
                    // 选区助手：润色期间把胶囊显示出来（SelectionPolishNotice），
                    // 出结果后留 2.5 秒给用户看一眼再收起。
                    const QString phase = inner.value("state").toString();
                    const bool active = phase == "polishing" || phase == "transcribing";
                    const bool terminal =
                        phase == "done" || phase == "replaced" || phase == "applied" ||
                        phase == "noSelection" || phase == "cancelled" || phase == "failed" ||
                        phase == "error";
                    if (active) {
                        capsuleHideTimer->stop();
                        capsule.setVisible(true);
                    } else if (terminal) {
                        capsuleHideTimer->start(2500);
                    }
                } else if (name == "qa:state") {
                    // QaStateEvent 的字段是 `kind`（activity/idle/thinking/…），
                    // 不是 phase/open。旧写法读不到值，导致浮层该显不显、该隐不隐。
                    const QString kind = inner.value("kind").toString();
                    qInfo("qa:state kind=%s", qPrintable(kind));
                    const bool idle = kind.isEmpty() || kind == "idle";
                    qa.setVisible(!idle);
                    // 有回答或思考中 ⇒ 展开；活动提示、录音中 ⇒ 收起成小条。
                    const bool expanded = kind == "thinking" || kind == "answer" ||
                                          kind == "answerDelta" || kind == "awaitingApproval";
                    qa.setExpanded(expanded);
                }
            }
        }
    });

    QObject::connect(&app, &QApplication::aboutToQuit, [backend]() {
        if (backend && backend->state() != QProcess::NotRunning) {
            backend->terminate();
            backend->waitForFinished(2000);
        }
    });

    const int exitCode = app.exec();
    // QtWebEngine 的析构在退出阶段会偶发 SIGSEGV（coredumpctl 里
    // openless-qt 的崩溃全在 __libc_start_main 收尾），表现为「退出时提示程序
    // 崩溃」。这里先收掉后端子进程，然后跳过整个析构链直接退出。
    if (backend && backend->state() != QProcess::NotRunning) {
        backend->terminate();
        backend->waitForFinished(2000);
    }
    std::_Exit(exitCode);
}
