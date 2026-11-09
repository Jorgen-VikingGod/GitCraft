# Getting GitCraft to Linux users

Besides the `.deb` and `.AppImage` on each GitHub release, the project publishes its own apt
repository on GitHub Pages.

## apt repository (automatic once set up)

`.github/workflows/apt-repo.yml` runs after "Release installers" finishes for a release. It takes
the `.deb` from the release, adds it to the repository kept on the `gh-pages` branch
(`packaging/apt/publish.sh`), signs it and publishes it on GitHub Pages. Only the newest five
versions are kept.

One-time setup:

1. Make a signing key that never expires, without a passphrase or with one you store as a secret:

       gpg --quick-generate-key "GitCraft apt <you@example.com>" rsa4096 sign never

2. Add two repository secrets (Settings > Secrets and variables > Actions):
   - `APT_GPG_PRIVATE_KEY`: the output of `gpg --armor --export-secret-keys <key id>`
   - `APT_GPG_PASSPHRASE`: its passphrase, or leave it out if there is none
3. Run the workflow once by hand (Actions > Publish apt repository > Run workflow, with a tag
   like `v0.3.0`). This creates the `gh-pages` branch.
4. Settings > Pages > Source "Deploy from a branch", branch `gh-pages`, folder `/ (root)`.

Until the secret exists the workflow only prints a warning and does nothing.

Keep the private key safe: if it is lost or replaced, everyone has to add the new public key again.

To try the script on your own machine without signing:

    APT_SKIP_SIGNING=1 packaging/apt/publish.sh /tmp/apt-test path/to/gitcraft_0.3.0_amd64.deb
