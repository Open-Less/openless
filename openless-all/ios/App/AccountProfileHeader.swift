import SwiftUI

extension AccountStatus {
  var profileLogin: String? {
    guard case .verifiedLogin(let login) = authentication else { return nil }
    return login
  }
}

struct AccountProfileHeader: View {
  let status: AccountStatus?
  var compact = false
  private var login: String? { status?.profileLogin }
  private var avatarURL: URL? {
    // Login has already passed AccountStatus's ASCII GitHub-name validation.
    login.flatMap { URL(string: "https://github.com/\($0).png?size=256") }
  }
  private var avatar: some View {
    AsyncImage(url: avatarURL) { image in
      image.resizable().scaledToFill()
    } placeholder: {
      Image(systemName: "person.fill")
        .font(.system(size: compact ? 20 : 38, weight: .light))
        .foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.quaternary)
    }
    .frame(width: compact ? 44 : 88, height: compact ? 44 : 88)
    .clipShape(Circle())
    .overlay(Circle().strokeBorder(Color.primary.opacity(0.08), lineWidth: compact ? 0.5 : 1))
    .accessibilityHidden(true)
  }
  var body: some View {
    if compact {
      HStack(spacing: 14) {
        avatar
        VStack(alignment: .leading, spacing: 2) {
          Text(login ?? "登录 OpenLess")
            .font(.headline).lineLimit(1).minimumScaleFactor(0.75).foregroundStyle(.primary)
            .accessibilityIdentifier("profileName")
          Text(login == nil ? "使用 GitHub 登录；同步暂不可用" : "我的主页")
            .font(.footnote).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        Spacer(minLength: 0)
      }.frame(minHeight: 56)
    } else {
      VStack(spacing: 12) {
        avatar
        Text(login ?? "欢迎使用 OpenLess")
          .font(.title2.weight(.semibold))
          .lineLimit(1).minimumScaleFactor(0.75).foregroundStyle(.primary)
          .accessibilityIdentifier("profileName")
        Text(login == nil ? "登录 GitHub，开启你的主页" : "GitHub 账户")
          .font(.subheadline).foregroundStyle(.secondary)
      }
      .frame(maxWidth: .infinity).padding(.vertical, 24)
      .background {
        LinearGradient(colors: [AppDesign.accent.opacity(0.08), .clear],
          startPoint: .topLeading, endPoint: .bottomTrailing)
      }
      .clipShape(RoundedRectangle(cornerRadius: AppDesign.Radius.card, style: .continuous))
    }
  }
}
