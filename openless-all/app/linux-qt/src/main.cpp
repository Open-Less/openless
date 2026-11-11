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
#include <QCommandLineParser>
#include <QElapsedTimer>
#include <QMenu>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProcess>
#include <QProcessEnvironment>
#include <QFileInfo>
#include <QScreen>
#include <QSystemTrayIcon>
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
    void javaScriptConsoleMessage(JavaScriptConsoleMessageLevel level, const QString &message,
                                 int line, const QString &source) override {
        if (level == InfoMessageLevel && !message.contains("openless", Qt::CaseInsensitive)) {
            return;
        }
        qInfo("[page %d] %s (%s:%d)", static_cast<int>(level), qPrintable(message),
              qPrintable(source), line);
    }
};

/// 宿主注入：让前端知道渲染层是 Chromium，且不要画二次标题栏。
const char *kHostScript =
    "window.__openlessHost='qt';"
    "(function(){var d=document.documentElement;"
    "if(d){d.dataset.olEngine='chromium';d.dataset.olPlatform='linux';}"
    "else{document.addEventListener('DOMContentLoaded',function(){"
    "var d=document.documentElement;d.dataset.olEngine='chromium';"
    "d.dataset.olPlatform='linux';});}})();";

enum class Surface { Main, Capsule, Qa };

/// 一个 webview 窗口。胶囊/问答用 layer-shell，主窗口是普通窗口。
class SurfaceWindow {
public:
    SurfaceWindow(Surface kind, const QString &baseUrl, QObject *parent);
    ~SurfaceWindow();

    QWebEngineView *view() const { return view_; }
    void load();
    void setVisible(bool visible);
    void setExpanded(bool expanded);
    bool isVisible() const { return visible_; }
    Surface kind() const { return kind_; }

private:
    void applyLayerShell();

    Surface kind_;
    QString url_;
    QWebEngineView *view_ = nullptr;
    bool visible_ = false;
    bool expanded_ = false;
    QSize compactSize_;
    QSize expandedSize_;
};

SurfaceWindow::SurfaceWindow(Surface kind, const QString &baseUrl, QObject *parent)
    : kind_(kind), view_(nullptr) {
    view_ = new QWebEngineView();
    view_->setPage(new LoggingPage(view_));

    auto *settings = view_->settings();
    settings->setAttribute(QWebEngineSettings::JavascriptEnabled, true);
    settings->setAttribute(QWebEngineSettings::LocalContentCanAccessRemoteUrls, true);

    QString windowKind;
    switch (kind) {
        case Surface::Main:
            windowKind = "main";
            view_->setWindowFlags(Qt::Window);
            view_->setWindowTitle("OpenLess");
            view_->resize(1180, 820);
            compactSize_ = QSize(1180, 820);
            break;
        case Surface::Capsule:
            windowKind = "capsule";
            view_->setWindowFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint | Qt::Tool);
            view_->setAttribute(Qt::WA_TranslucentBackground, true);
            view_->setAttribute(Qt::WA_NoSystemBackground, true);
            view_->setStyleSheet("background: transparent;");
            view_->page()->setBackgroundColor(Qt::transparent);
            compactSize_ = QSize(460, 228);
            break;
        case Surface::Qa:
            windowKind = "qa";
            view_->setWindowFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint | Qt::Tool);
            view_->setAttribute(Qt::WA_TranslucentBackground, true);
            view_->setAttribute(Qt::WA_NoSystemBackground, true);
            view_->setStyleSheet("background: transparent;");
            view_->page()->setBackgroundColor(Qt::transparent);
            compactSize_ = QSize(480, 80);
            expandedSize_ = QSize(760, 416);
            break;
    }
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
        url_ = QString("%1/index.html?bridge=http&window=%2").arg(baseUrl, windowKind);
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

SurfaceWindow::~SurfaceWindow() { delete view_; }

void SurfaceWindow::load() {
    if (kind_ != Surface::Main) {
        // layer-shell 必须在**平台窗口创建之前**安装集成：
        // `winId()` 会提前创建平台窗口（xdg-shell 已绑定），所以改用
        // `windowHandle()` —— 只创建 QWindow，真正的 surface 延迟到 show()。
        view_->winId();
        applyLayerShell();
        // LayerShellQt 的集成只在 surface 创建时生效：配置完要 hide + show
        // 让平台 surface 按 layer-shell 重建（否则会报 already has a shell
        // integration，或干脆不显示）。
        view_->hide();
        view_->show();
    }
    view_->load(QUrl(url_));
    qInfo("surface=%d url=%s", static_cast<int>(kind_), qPrintable(url_));
}

void SurfaceWindow::applyLayerShell() {
    auto *window = view_->windowHandle();
    if (!window) return;
    auto *layerWindow = LayerShellQt::Window::get(window);
    if (!layerWindow) {
        qInfo("surface=%d layer-shell 不可用（非 Wayland？）", static_cast<int>(kind_));
        return;
    }
    layerWindow->setLayer(LayerShellQt::Window::LayerOverlay);
    layerWindow->setExclusiveZone(-1);
    if (kind_ == Surface::Capsule) {
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

void SurfaceWindow::setVisible(bool visible) {
    if (visible_ == visible) return;
    visible_ = visible;
    if (visible) {
        view_->show();
    } else {
        view_->hide();
    }
}

void SurfaceWindow::setExpanded(bool expanded) {
    if (kind_ != Surface::Qa || expanded_ == expanded) return;
    expanded_ = expanded;
    view_->resize(expanded ? expandedSize_ : compactSize_);
}

}  // namespace

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    app.setApplicationName("OpenLess");
    app.setApplicationDisplayName("OpenLess");
    app.setQuitOnLastWindowClosed(false);

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
        backend->setProcessEnvironment(backendEnv);
        // 默认与自身同目录（安装后是 /usr/lib/openless），这样解包试跑也能找到后端。
        QString defaultHost = QCoreApplication::applicationDirPath() + "/openless-linux-web";
        if (!QFileInfo::exists(defaultHost)) {
            defaultHost = QStringLiteral("/usr/lib/openless/openless-linux-web");
        }
        const QString hostBin = qEnvironmentVariable("OPENLESS_HOST_BIN", defaultHost);
        QStringList args;
        backend->start(hostBin, args);
        if (!backend->waitForStarted(5000)) {
            qCritical("无法启动后端 %s", qPrintable(hostBin));
            return 1;
        }
        QElapsedTimer timer;
        timer.start();
        while (timer.elapsed() < 20000 && baseUrl.isEmpty()) {
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
    mainWindow.load();
    capsule.load();
    qa.load();
    mainWindow.setVisible(qEnvironmentVariableIsSet("OPENLESS_QT_SHOW_MAIN") ||
                          !qEnvironmentVariableIsSet("OPENLESS_QT_HIDE_MAIN"));
    if (qEnvironmentVariableIsSet("OPENLESS_QT_SHOW_CAPSULE")) capsule.setVisible(true);
    if (qEnvironmentVariableIsSet("OPENLESS_QT_SHOW_QA")) qa.setVisible(true);

    // ── 3. 托盘 ─────────────────────────────────────────────────────────────
    auto *tray = new QSystemTrayIcon(&app);
    tray->setIcon(QIcon::fromTheme("openless",
                                   QIcon::fromTheme("audio-input-microphone")));
    auto *menu = new QMenu();
    menu->addAction("打开 OpenLess", [&mainWindow]() { mainWindow.setVisible(true); });
    menu->addAction("隐藏窗口", [&mainWindow]() { mainWindow.setVisible(false); });
    menu->addSeparator();
    menu->addAction("退出", []() { QCoreApplication::quit(); });
    tray->setContextMenu(menu);
    tray->show();

    // ── 4. 事件：dictation:state 控制胶囊，qa:state 控制问答面板 ────────────
    auto *manager = new QNetworkAccessManager(&app);
    auto *reply = manager->get(QNetworkRequest(QUrl(baseUrl + "/api/events")));
    auto *buffer = new QByteArray();
    QObject::connect(reply, &QNetworkReply::readyRead, &app, [reply, buffer, &capsule, &qa]() {
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
                    const QString phase = inner.value("phase").toString();
                    const bool active = !phase.isEmpty() && phase != "idle";
                    capsule.setVisible(active);
                } else if (name == "qa:state") {
                    const QString phase = inner.value("phase").toString();
                    const bool open = inner.value("open").toBool(false) ||
                                      (!phase.isEmpty() && phase != "idle" && phase != "hidden");
                    qa.setVisible(open);
                    qa.setExpanded(inner.value("expanded").toBool(false));
                }
            }
        }
    });

    QObject::connect(&app, &QApplication::aboutToQuit, [backend]() {
        if (backend) {
            backend->terminate();
            backend->waitForFinished(2000);
        }
    });

    return app.exec();
}
