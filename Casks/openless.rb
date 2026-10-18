cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "2.0.1+build.20261016"
  sha256 arm:   "54de2717b094f71a9fd5aa2e73598f565b930668f92baa5b0b3e5d2236e7eaae",
         intel: "c28b80e0275ae6aa0d9884bcfc84a89f22df314fc6a3fd43965e33af40743bf3"

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
