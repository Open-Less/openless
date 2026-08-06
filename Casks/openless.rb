cask "openless" do
  arch arm: "aarch64", intel: "x64"

  version "1.3.16"
  sha256 arm:   "2cb55858b1c2104ac4ae818edfac1f125e7eb8547283fabcf756276548fed5ca",
         intel: "bd86763ac3226fd90e9206766539bd5a986e1421ac01e94a45a0027b2d1d252d"

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
