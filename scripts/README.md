# scripts

Helper scripts. Each one says what it is for at the top of the file.

`shot.swift` screenshots the display through ScreenCaptureKit, because `screencapture` reports the overlay's transparent pixels as opaque white. Use `swift scripts/shot.swift --out shot.png [--region x,y,w,h] [--hide-overlay]`.
