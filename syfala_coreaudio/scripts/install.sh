NAME="syfala_coreaudio"
LIBRARY_PATH=$1
# replace with your own signature
TEAM_ID="8SHPR83B3J"

# the UUID of our plugin factory
# you can also use uuidgen
FACTORY_UUID=67FDAB5A-2429-478E-B0CE-61F1C23A03C6

mkdir -p $NAME.driver/Contents/{MacOS,Resources}

cat > "$NAME.driver/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
        <string>com.emeraude.$NAME</string>
    <key>CFBundleName</key>
        <string>$NAME</string>
    <key>CFBundleExecutable</key>
        <string>$NAME</string>
    <key>AudioServerPlugin_Network</key>
        <true/>
    <key>CFBundleSupportedPlatforms</key>
        <array>
            <string>MacOSX</string>
        </array>
    <key>CFPlugInFactories</key>
        <dict>
            <key>$FACTORY_UUID</key>
                <string>create</string>
        </dict>
    <key>CFPlugInTypes</key>
        <dict>
            <key>443ABAB8-E7B3-491A-B985-BEB9187030DB</key>
            <array>
                <string>$FACTORY_UUID</string>
            </array>
        </dict>
</dict>
</plist>
EOF

cp $LIBRARY_PATH $NAME
mv $NAME $NAME.driver/Contents/MacOS

codesign --force --deep --options runtime --sign "$TEAM_ID" "$NAME.driver"

DEST_DIR="/Library/Audio/Plug-Ins/HAL"

sudo rm -rf "$DEST_DIR/$NAME.driver"
sudo mv $NAME.driver "$DEST_DIR/"

sudo killall -9 coreaudiod