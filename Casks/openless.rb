cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "1.3.15"
  sha256 arm:   "206a0189af6876d727fcdc8ec50c362d20721080f3ce0904ec683fa3cd3d8414",
         intel: "b5502dc1bb8b86c42158df767e61c7aa380ea12ae3a11f4be6fef625e54fc42b"

  url "https://github.com/Open-Less/openless/releases/download/v#{version}-tauri/OpenLess_#{version}_#{arch}.dmg"
  name "OpenLess"
  desc "Menu-bar voice input layer"
  homepage "https://github.com/Open-Less/openless"

  livecheck do
    url :url
    regex(/^v?(\d+(?:\.\d+)+)[._-]tauri$/i)
  end

  auto_updates true
  depends_on macos: :monterey

  app "OpenLess.app"

  zap trash: [
    "~/Library/Application Support/OpenLess",
    "~/Library/Caches/com.openless.app",
    "~/Library/Logs/OpenLess",
    "~/Library/Preferences/com.openless.app.plist",
    "~/Library/WebKit/com.openless.app",
  ]
end
