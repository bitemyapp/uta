#!/usr/bin/env bash
# Install a package built on another Nix machine without importing unsigned
# executable output into the local store. Runtime dependencies must exist locally.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo 'Usage: scripts/install-local.sh PACKAGE_DIRECTORY RUNTIME_REFERENCES_FILE' >&2
  exit 2
fi
package=$(realpath "$1")
references=$(realpath "$2")
data_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}
install_base="$data_dir/aizome/uta"
binary="$package/bin/.aizome-uta-wrapped"
[[ -x "$binary" && -f "$references" ]]
revision=$(sha256sum "$binary" | cut -c1-16)
destination="$install_base/builds/$revision"
mkdir -p "$destination" "$HOME/.local/bin" "$data_dir/applications" "$data_dir/icons/hicolor/scalable/apps"
cp -a "$package/." "$destination/"
chmod -R u+w "$destination"
# Preserve the Nix wrapper's runtime environment; resolve the executable beside it.
sed -i '/^exec -a /c\exec -a "$0" "${BASH_SOURCE[0]%/*}/.aizome-uta-wrapped" "$@"' "$destination/bin/aizome-uta"
mkdir -p "$destination/runtime-roots"
while IFS= read -r reference; do
  [[ "$reference" =~ ^/nix/store/[a-z0-9]{32}-[^/]+$ ]] || { echo 'Invalid runtime reference' >&2; exit 1; }
  nix-store --add-root "$destination/runtime-roots/${reference##*/}" --indirect --realise "$reference" >/dev/null
done < "$references"
cp "$references" "$destination/runtime-references.txt"
ln -sfn "$destination" "$install_base/current.next"
mv -Tf "$install_base/current.next" "$install_base/current"
# A launcher (not a symlink) lets the relocated wrapper find its sibling ELF.
printf '#!/usr/bin/env bash\nexec %q "$@"\n' "$install_base/current/bin/aizome-uta" > "$HOME/.local/bin/.aizome-uta.next"
chmod 755 "$HOME/.local/bin/.aizome-uta.next"
mv -Tf "$HOME/.local/bin/.aizome-uta.next" "$HOME/.local/bin/aizome-uta"
sed "s|^Exec=.*|Exec=\"$HOME/.local/bin/aizome-uta\" %U|" \
  "$destination/share/applications/org.aizome.Uta.desktop" > "$data_dir/applications/org.aizome.Uta.desktop"
cp "$destination/share/icons/hicolor/scalable/apps/org.aizome.Uta.svg" "$data_dir/icons/hicolor/scalable/apps/org.aizome.Uta.svg"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$data_dir/applications"; fi
"$HOME/.local/bin/aizome-uta" --version
printf 'Installed in %s\n' "$destination"
