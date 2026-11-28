cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "2.0.2+build.20261115"
  sha256 arm:   "98fb622de90d2c197e9376f8e89e603d6be3ceaafcbb430de99befead550d4e8",
         intel: "27304197aec7c2fc391f92fb417749a837aba90419e569e44e416fbaf8cfc072"

  url "https://github.com/Open-Less/openless/releases/download/v#{version.to_s.split("+", 2).first}-tauri/OpenLess_#{version}_#{arch}.dmg"
  name "OpenLess"
  desc "Menu-bar voice input layer"
  homepage "https://github.com/Open-Less/openless"

  livecheck do
    url :url
    regex(/^v?(\d+(?:\.\d+)+)[._-]tauri$/i)
  end

  auto_updates true
  depends_on macos: (version.major.to_i >= 2 ? :sonoma : :monterey)

  app "OpenLess.app"

  zap trash: [
    "~/Library/Application Support/OpenLess",
    "~/Library/Caches/com.openless.app",
    "~/Library/Logs/OpenLess",
    "~/Library/Preferences/com.openless.app.plist",
    "~/Library/WebKit/com.openless.app",
  ]
end
