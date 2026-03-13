#!/bin/bash
# Phonix macOS installer helper
# Strips quarantine flags and moves the app to /Applications

APP_NAME="Phonix.app"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_PATH="$SCRIPT_DIR/$APP_NAME"

if [ ! -d "$APP_PATH" ]; then
    echo "Error: $APP_NAME not found in $(dirname "$0")"
    exit 1
fi

echo "Installing Phonix..."

# Remove quarantine flag (prevents Gatekeeper "damaged" error)
xattr -cr "$APP_PATH"

# Copy to Applications
cp -R "$APP_PATH" /Applications/
xattr -cr "/Applications/$APP_NAME"

echo ""
echo "Phonix installed to /Applications."
echo ""
echo "IMPORTANT: On first launch you must grant two permissions:"
echo "  1. Microphone - prompted automatically"
echo "  2. Accessibility - go to System Settings > Privacy & Security > Accessibility"
echo "     and enable Phonix. Then restart the app."
echo ""
echo "You can now launch Phonix from your Applications folder."
