// SPDX-License-Identifier: AGPL-3.0-only
#include <QApplication>
#include <QAction>
#include <QDBusConnection>
#include <QDBusContext>
#include <QDBusMessage>
#include <QDBusInterface>
#include <QDBusReply>
#include <QDBusObjectPath>
#include <QDateTime>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeySequence>
#include <QQueue>
#include <QTimer>
#include <QUuid>
#include <QFileInfo>
#include <QDir>
#include <KGlobalAccel>

class Bridge : public QObject, protected QDBusContext {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.openless.Desktop1")
    QMap<QString,QAction*> actions;
    QMap<QString,QJsonObject> bindings;
    QMap<QString,QDBusMessage> acknowledgements;
    QQueue<QJsonObject> commands;
    QDBusMessage waiting;
    quint64 waiterGeneration=0;
    QString snapshot="{}";
    qint64 snapshotTime=0;
    QString releasePath;
    static QString encode(const QJsonObject &value) { return QString::fromUtf8(QJsonDocument(value).toJson(QJsonDocument::Compact)); }
    void replyCommand() {
        if (waiting.type()!=QDBusMessage::MethodCallMessage || commands.isEmpty()) return;
        QDBusConnection::sessionBus().send(waiting.createReply(encode(commands.dequeue())));
        waiting=QDBusMessage(); ++waiterGeneration;
    }
    bool queue(QJsonObject command) {
        if (!calledFromDBus() || QDateTime::currentMSecsSinceEpoch()-snapshotTime>5000) return false;
        const QString id=QUuid::createUuid().toString(QUuid::WithoutBraces);
        command["id"]=id;
        setDelayedReply(true); acknowledgements[id]=message(); commands.enqueue(command); replyCommand();
        QTimer::singleShot(1500,this,[this,id](){ Ack(id,false); });
        return false; // delayed reply contains the actual KWin acknowledgement
    }
    static QKeySequence key(const QJsonObject &binding) {
        QString accelerator=binding["accelerator"].toString();
        accelerator.replace("<Control>","Ctrl+").replace("<Shift>","Shift+").replace("<Alt>","Alt+").replace("<Super>","Meta+");
        accelerator.replace("Control_R","Ctrl").replace("Control_L","Ctrl").replace("Alt_R","Alt").replace("Alt_L","Alt");
        return QKeySequence(accelerator);
    }
public:
    Bridge() = default;
public slots:
    void Released(const QString &name,qlonglong) {const auto binding=bindings.value(name);if(!binding.isEmpty())emit Hotkey(binding["action"].toString(),binding["symbol"].toInt(),binding["states"].toInt(),false);}
    uint Version() const { return 1; }
    QString Snapshot() const { return QDateTime::currentMSecsSinceEpoch()-snapshotTime<5000?snapshot:"{}"; }
    void Update(const QString &json) {
        const auto document=QJsonDocument::fromJson(json.toUtf8());
        if (document.isObject() && document.object()["version"].toInt()==1) { snapshot=json; snapshotTime=QDateTime::currentMSecsSinceEpoch(); }
    }
    QString Bind(const QString &json) {
        const auto document=QJsonDocument::fromJson(json.toUtf8());
        if (!document.isArray() || document.array().size()>64) return "Invalid desktop binding document";
        QMap<QString,QJsonObject> next;
        for (const auto value:document.array()) {
            const auto binding=value.toObject(); const auto sequence=key(binding);
            const QString id=binding["action"].toString()+":"+QString::number(binding["symbol"].toInt())+":"+QString::number(binding["states"].toInt());
            bool owned=false; for (auto action:actions) if (KGlobalAccel::self()->shortcut(action).contains(sequence)) owned=true;
            if (sequence.isEmpty() || (!owned&&!KGlobalAccel::isGlobalShortcutAvailable(sequence,"openless-desktop-bridge"))) return "Shortcut conflict: "+binding["accelerator"].toString();
            next[id]=binding;
        }
        const auto previous=bindings;
        for (auto action:actions) KGlobalAccel::self()->removeAllShortcuts(action);
        bool success=true;
        for (auto it=next.begin();it!=next.end();++it) {
            auto action=actions.value(it.key());
            if (!action) {
                action=new QAction(it.key(),this);action->setObjectName(it.key());actions[it.key()]=action;
                connect(action,&QAction::triggered,this,[this,action](){const auto binding=bindings.value(action->objectName());if(!binding.isEmpty())emit Hotkey(binding["action"].toString(),binding["symbol"].toInt(),binding["states"].toInt(),true);});
            }
            if (!KGlobalAccel::self()->setShortcut(action,{key(it.value())},KGlobalAccel::NoAutoloading)) { success=false;break; }
        }
        if (!success) {
            for (auto action:actions) KGlobalAccel::self()->removeAllShortcuts(action);
            for (auto it=previous.begin();it!=previous.end();++it) KGlobalAccel::self()->setShortcut(actions[it.key()],{key(it.value())},KGlobalAccel::NoAutoloading);
            return "Shortcut registration failed; previous bindings restored";
        }
        bindings=next;
        if(releasePath.isEmpty()) {
            QDBusInterface service("org.kde.kglobalaccel","/kglobalaccel","org.kde.KGlobalAccel");
            QDBusReply<QDBusObjectPath> component=service.call("getComponent","openless-desktop-bridge");
            if(component.isValid()) {
                releasePath=component.value().path();
                QDBusConnection::sessionBus().connect("org.kde.kglobalaccel",releasePath,"org.kde.kglobalaccel.Component","globalShortcutReleased",this,SLOT(Released(QString,qlonglong)));
            }
        }
        return {};
    }
    bool Restore(const QString &target) { return queue({{"op","restore"},{"target",target}}); }
    bool Place(const QString &title,int x,int y) { if(!title.startsWith("OpenLess "))return false;return queue({{"op","place"},{"title",title},{"x",x},{"y",y}}); }
    QString Next() {
        if (!commands.isEmpty()) return encode(commands.dequeue());
        if (waiting.type()==QDBusMessage::MethodCallMessage) return "{}";
        setDelayedReply(true);waiting=message();const auto generation=++waiterGeneration;
        QTimer::singleShot(1000,this,[this,generation](){if(waiterGeneration==generation&&waiting.type()==QDBusMessage::MethodCallMessage){QDBusConnection::sessionBus().send(waiting.createReply(QString("{}")));waiting=QDBusMessage();}});
        return {};
    }
    void Ack(const QString &id,bool success) {
        if (!acknowledgements.contains(id)) return;
        QDBusConnection::sessionBus().send(acknowledgements.take(id).createReply(success));
    }
signals:
    void Hotkey(const QString &action,uint symbol,uint states,bool pressed);
};

int main(int argc,char **argv) {
    // This helper only uses D-Bus; a private offscreen platform plugin avoids
    // loading Qt plugins from an incompatible Plasma installation.
    qputenv("QT_QPA_PLATFORM", "offscreen");
    const auto plugins=QFileInfo(QString::fromLocal8Bit(argv[0])).absoluteDir().filePath("plugins/platforms");
    if(QDir(plugins).exists())qputenv("QT_QPA_PLATFORM_PLUGIN_PATH",plugins.toUtf8());
    QApplication app(argc,argv);app.setApplicationName("openless-desktop-bridge");app.setQuitOnLastWindowClosed(false);
    Bridge bridge;
    auto bus=QDBusConnection::sessionBus();
    if(!bus.registerService("org.openless.Desktop1") || !bus.registerObject("/org/openless/Desktop1",&bridge,QDBusConnection::ExportAllSlots|QDBusConnection::ExportAllSignals))return 1;
    return app.exec();
}
#include "bridge.moc"
