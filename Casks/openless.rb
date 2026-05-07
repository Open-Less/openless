cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "1.2.23"
  sha256 arm:   "c6544555150459ba616ba43b27e226a88da40c5349c8e19f71a0bedfce7050b4",
         intel: "ffcf84f5510ffe369277ddf9980b930fb5fb72a5e799020af4cf4720905625e1"

  url "https://github.com/appergb/openless/releases/download/v#{version}-tauri/OpenLess_#{version}_#{arch}.dmg"
  name "OpenLess"
  desc "Menu-bar voice input layer for macOS"
  homepage "https://github.com/appergb/openless"

  livecheck do
    url :url
    regex(/^v?(\d+(?:\.\d+)+)[._-]tauri$/i)
  end

  auto_updates true

  app "OpenLess.app"

  zap trash: [
    "~/Library/Application Support/OpenLess",
    "~/Library/Caches/com.openless.app",
    "~/Library/Logs/OpenLess",
    "~/Library/Preferences/com.openless.app.plist",
    "~/Library/WebKit/com.openless.app",
  ]
end
