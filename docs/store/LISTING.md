# Browser extension store listing

The text to paste into the Chrome Web Store, Microsoft Edge Add-ons, Opera add-ons and
addons.mozilla.org. Upload the zips that `node tools/pack-extension.mjs` writes to
`build-ext/store/`.

## Name

Ultimate Download Manager

## Short description (up to 132 characters)

Send downloads and videos from your browser to Ultimate Download Manager for fast, resumable downloading.

## Description

Ultimate Download Manager takes over the downloads you start in your browser and finishes them
faster, with many connections per file, pause and resume, and a queue.

With the extension:
- Downloads you click go straight to Ultimate Download Manager, which asks where to save each one
- A download button appears over videos, to save them in the quality you choose
- Right-click a link, picture or video to download it, or download every link on a page
- Downloads that need you to be signed in still work: the browser's cookies for that site go
  with the download
- Sites you choose can stay with the browser's own downloader

The extension needs the free Ultimate Download Manager app for Windows. Nothing is sent
anywhere except to that app on your computer.

## Category

Productivity (Chrome, Edge, Opera); Download Management (Firefox)

## Single purpose (Chrome Web Store)

Hand downloads started in the browser to the Ultimate Download Manager app on the same computer.

## Why each permission is needed

| Permission | Why |
|---|---|
| downloads | To take over downloads the browser starts and cancel the browser's copy |
| nativeMessaging | To pass each download to the Ultimate Download Manager app on this computer |
| cookies | To send the site's cookies with a download, so files behind a sign-in download too |
| contextMenus | For "Download with Ultimate Download Manager" in the right-click menu |
| storage | To remember the extension's settings (sites to leave alone, the paired computer) |
| webRequest | To see a download's form data and headers, so it can be repeated exactly by the app |
| declarativeNetRequestWithHostAccess | To make file types you chose download instead of opening in a tab |
| scripting | To find the videos and links on the page you are on, when you ask for them |
| proxy | To read the browser's proxy setting, so the app downloads through the same proxy |
| Host access to all sites | Downloads and videos can come from any site |

## Data use disclosure

- Collects: none. The extension handles download addresses, cookies for the download's site and
  page titles, and only passes them to the app on the user's computer.
- Sold or shared with third parties: no.
- Used for anything other than the single purpose: no.

## Privacy policy link

The `PRIVACY.md` file on the project's GitHub page.
