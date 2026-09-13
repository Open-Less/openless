import UIKit

final class KeyboardViewController: UIInputViewController, UITableViewDataSource, UITableViewDelegate {
    private let tableView = UITableView(frame: .zero, style: .plain)
    private let statusLabel = UILabel()
    private let emptyLabel = UILabel()
    private let globeButton = UIButton(type: .system)
    private var clips: [KeyboardClip] = []
    private var heightConstraint: NSLayoutConstraint?
    private let accent = UIColor(red: 37 / 255, green: 99 / 255, blue: 235 / 255, alpha: 1)

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemGroupedBackground
        let title = UILabel()
        title.text = "OpenLess"
        title.font = .preferredFont(forTextStyle: .headline)
        title.adjustsFontForContentSizeCategory = true
        let refresh = button(title: "刷新", symbol: "arrow.clockwise", action: #selector(refreshTapped))
        let header = UIStackView(arrangedSubviews: [title, UIView(), refresh])
        header.alignment = .center
        header.spacing = 12
        statusLabel.text = "点击一条文字，插入当前输入框"
        statusLabel.font = .preferredFont(forTextStyle: .caption1)
        statusLabel.textColor = .secondaryLabel
        statusLabel.numberOfLines = 2
        statusLabel.adjustsFontForContentSizeCategory = true

        tableView.backgroundColor = .clear
        tableView.dataSource = self
        tableView.delegate = self
        tableView.rowHeight = UITableView.automaticDimension
        tableView.estimatedRowHeight = 82
        tableView.register(UITableViewCell.self, forCellReuseIdentifier: "clip")
        tableView.layer.cornerRadius = 12
        tableView.clipsToBounds = true
        emptyLabel.font = .preferredFont(forTextStyle: .subheadline)
        emptyLabel.textColor = .secondaryLabel
        emptyLabel.textAlignment = .center
        emptyLabel.numberOfLines = 0
        emptyLabel.adjustsFontForContentSizeCategory = true

        globeButton.setImage(UIImage(systemName: "globe"), for: .normal)
        globeButton.accessibilityLabel = "切换键盘，长按选择输入法"
        globeButton.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)
        let space = button(title: "空格", symbol: nil, action: #selector(insertSpace))
        let newline = button(title: "换行", symbol: "return", action: #selector(insertNewline))
        let delete = button(title: nil, symbol: "delete.left", action: #selector(deleteBackward))
        delete.accessibilityLabel = "删除前一个字符"
        let hide = button(title: nil, symbol: "keyboard.chevron.compact.down", action: #selector(hideKeyboard))
        hide.accessibilityLabel = "收起键盘"
        let controls = UIStackView(arrangedSubviews: [globeButton, space, newline, delete, hide])
        controls.axis = .horizontal
        controls.distribution = .fillEqually
        controls.spacing = 8
        for control in controls.arrangedSubviews {
            control.backgroundColor = .secondarySystemGroupedBackground
            control.layer.cornerRadius = 9
            control.tintColor = accent
            control.heightAnchor.constraint(greaterThanOrEqualToConstant: 44).isActive = true
        }

        let stack = UIStackView(arrangedSubviews: [header, statusLabel, tableView, controls])
        stack.axis = .vertical
        stack.spacing = 10
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 12),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -12),
            stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 10),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -8)
        ])
        header.setContentHuggingPriority(.required, for: .vertical)
        statusLabel.setContentHuggingPriority(.required, for: .vertical)
        controls.setContentHuggingPriority(.required, for: .vertical)
        reloadClips()
    }

    override func updateViewConstraints() {
        if heightConstraint == nil {
            let constraint = view.heightAnchor.constraint(equalToConstant: 340)
            constraint.priority = UILayoutPriority(999)
            constraint.isActive = true
            heightConstraint = constraint
        }
        heightConstraint?.constant = traitCollection.verticalSizeClass == .compact ? 250 : 340
        super.updateViewConstraints()
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        reloadClips()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        globeButton.isHidden = !needsInputModeSwitchKey
    }

    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        if !hasFullAccess && !clips.isEmpty { reloadClips() }
    }

    private func button(title: String?, symbol: String?, action: Selector) -> UIButton {
        let button = UIButton(type: .system)
        var configuration = UIButton.Configuration.plain()
        configuration.title = title
        configuration.image = symbol.flatMap { UIImage(systemName: $0) }
        configuration.imagePadding = 6
        configuration.baseForegroundColor = accent
        button.configuration = configuration
        button.addTarget(self, action: action, for: .touchUpInside)
        return button
    }

    private func reloadClips() {
        guard isViewLoaded else { return }
        guard hasFullAccess else {
            clips = []
            emptyLabel.text = "请在系统设置中为 OpenLess 键盘\n开启“允许完全访问”，以读取已发送的文字。"
            statusLabel.text = "共享文字需要键盘的完全访问权限"
            tableView.backgroundView = emptyLabel
            tableView.reloadData()
            return
        }
        do {
            clips = try KeyboardStore.read()
            emptyLabel.text = "先在 OpenLess 中完成听写，\n点击“发送到键盘”，再回到这里刷新。"
            statusLabel.text = clips.isEmpty ? "暂时没有发送到键盘的文字" : "点击一条文字，插入当前输入框"
        } catch {
            clips = []
            emptyLabel.text = "暂时无法读取共享文字。\n请打开 OpenLess 后重新发送。"
            statusLabel.text = "共享文字读取失败"
        }
        tableView.backgroundView = clips.isEmpty ? emptyLabel : nil
        tableView.reloadData()
    }

    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int { clips.count }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "clip", for: indexPath)
        let clip = clips[indexPath.row]
        var content = cell.defaultContentConfiguration()
        content.text = clip.text
        content.textProperties.numberOfLines = 3
        content.textProperties.font = .preferredFont(forTextStyle: .subheadline)
        content.secondaryText = "\(clip.styleName) · \(clip.text.count) 字 · \(clip.createdAt.formatted(date: .omitted, time: .shortened))"
        content.secondaryTextProperties.color = .secondaryLabel
        content.secondaryTextProperties.font = .preferredFont(forTextStyle: .caption2)
        content.image = UIImage(systemName: "arrow.turn.down.left")
        content.imageProperties.tintColor = accent
        cell.contentConfiguration = content
        cell.backgroundColor = .secondarySystemGroupedBackground
        cell.accessibilityLabel = "插入：\(clip.text)"
        cell.accessibilityTraits = .button
        return cell
    }

    func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        guard hasFullAccess, clips.indices.contains(indexPath.row) else { reloadClips(); return }
        textDocumentProxy.insertText(clips[indexPath.row].text)
        statusLabel.text = "已发送插入请求，可在输入框中继续编辑"
        UIAccessibility.post(notification: .announcement, argument: "已插入所选文字")
    }

    @objc private func refreshTapped() { reloadClips() }
    @objc private func insertSpace() { textDocumentProxy.insertText(" ") }
    @objc private func insertNewline() { textDocumentProxy.insertText("\n") }
    @objc private func deleteBackward() { textDocumentProxy.deleteBackward() }
    @objc private func hideKeyboard() { dismissKeyboard() }
}
