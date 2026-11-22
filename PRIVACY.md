# Privacy policy

This policy covers Ultimate Download Manager, the Windows app, and its browser extension.

## What the extension does with your data

The extension passes downloads you start in the browser to the Ultimate Download Manager app
running on the same computer. For each download it hands over:

- the download's address, and the page it came from,
- the browser's cookies for that site and its user agent, so the app can download files that
  need you to be signed in,
- for a video on a page, the page's address and title.

This goes from the browser to the app on your computer through the browser's native messaging.
It is not sent anywhere else.

If you pair the extension with the app on another computer in your home network ("Send to
another computer"), downloads go to that computer instead, over your local network only.

## What is not collected

- No analytics, no tracking and no advertising.
- Nothing is sent to the developers or to any third party.
- Your browsing history is not read or stored. The extension looks only at downloads, at
  links you choose to download, and at the videos on the page you are viewing.

## What the app stores

The app keeps its download list, settings and logs on your computer, in
`%APPDATA%\udm`. Cookies the extension hands over for a download are kept in memory only and are
never written to disk. Website sign-in sessions you choose to save are encrypted with Windows'
own data protection.

## Contact

Questions about this policy: open an issue on the project's GitHub page.
