#!/usr/bin/env swift
//
// Screenshot a display through ScreenCaptureKit.
//
// `screencapture` is not usable for anything that involves the overlay window.
// `-l <overlay>` always returns the same 107711-byte empty image, because the
// webview's composited layer is not in that path, and a full-screen capture
// composites the overlay's fully transparent pixels as white. Measurements of
// the pointer (see #140, #141) depend on reading the real composite, so this
// goes through SCScreenshotManager with `shouldBeOpaque = false`.
//
// Usage:
//   swift scripts/shot.swift --out shot.png
//   swift scripts/shot.swift --out window.png --region 0,33,900,640
//   swift scripts/shot.swift --out behind.png --hide-overlay
//
//   --out <path>          where to write the PNG (required)
//   --region x,y,w,h      crop to a region in screen points
//   --hide-overlay        exclude OpenClicky's overlay window from the capture
//
// Exit codes: 0 wrote the file, 2 bad arguments, 3 capture failed.

import CoreGraphics
import Foundation
import ImageIO
import ScreenCaptureKit
import UniformTypeIdentifiers

func die(_ message: String, _ code: Int32) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(code)
}

var out: String?
var region: CGRect?
var hideOverlay = false
var displayIndex = 0

var args = Array(CommandLine.arguments.dropFirst())
while let flag = args.first {
    args.removeFirst()
    switch flag {
    case "--out":
        guard let value = args.first else { die("--out needs a path", 2) }
        args.removeFirst()
        out = value
    case "--region":
        guard let value = args.first else { die("--region needs x,y,w,h", 2) }
        args.removeFirst()
        let parts = value.split(separator: ",").compactMap { Double($0) }
        guard parts.count == 4 else { die("--region needs four numbers: x,y,w,h", 2) }
        // Coordinates are local to the chosen display, not to the desktop: the
        // capture only covers one screen.
        region = CGRect(x: parts[0], y: parts[1], width: parts[2], height: parts[3])
    case "--display":
        guard let value = args.first, let index = Int(value), index >= 0 else {
            die("--display needs a display index (0 is the first one CoreGraphics lists)", 2)
        }
        args.removeFirst()
        displayIndex = index
    case "--hide-overlay":
        hideOverlay = true
    default:
        die("unknown argument: \(flag)", 2)
    }
}

guard let out else { die("--out is required", 2) }

let semaphore = DispatchSemaphore(value: 0)
var status: Int32 = 3

Task {
    defer { semaphore.signal() }
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        guard displayIndex < content.displays.count else {
            die("--display \(displayIndex) does not exist; \(content.displays.count) display(s) found", 2)
        }
        let display = content.displays[displayIndex]

        var excluded: [SCWindow] = []
        if hideOverlay {
            excluded = content.windows.filter { $0.title == "overlay" }
            // A capture with the overlay left in it is the one thing this flag
            // exists to prevent, and it used to be reported as a success.
            if excluded.isEmpty {
                die("--hide-overlay found no window titled overlay; nothing was hidden", 3)
            }
        }

        let filter = SCContentFilter(display: display, excludingWindows: excluded)
        // The display's width is in points; the capture wants device pixels. That
        // factor is 2 on every retina panel and 1 on every other one, so ask
        // CoreGraphics instead of assuming the machine this was written on.
        let pixelWidth = CGDisplayPixelsWide(display.displayID)
        let scale = display.width > 0 ? Double(pixelWidth) / Double(display.width) : 1
        let config = SCStreamConfiguration()
        config.width = Int((Double(display.width) * scale).rounded())
        config.height = Int((Double(display.height) * scale).rounded())
        config.showsCursor = false
        // Without these two the overlay's alpha=0 pixels come out white, which is
        // exactly the false reading that sent #140's measurement sideways.
        config.shouldBeOpaque = false
        config.backgroundColor = .clear

        let full = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)

        let image: CGImage
        if let region {
            // The capture is in device pixels, the region is in screen points.
            let scale = CGFloat(full.width) / CGFloat(display.width)
            let scaled = CGRect(x: region.origin.x * scale,
                                y: region.origin.y * scale,
                                width: region.width * scale,
                                height: region.height * scale)
            guard let cropped = full.cropping(to: scaled.integral) else {
                die("region \(region) is outside the \(full.width)x\(full.height) capture", 3)
            }
            image = cropped
        } else {
            image = full
        }

        guard let dest = CGImageDestinationCreateWithURL(
            URL(fileURLWithPath: out) as CFURL,
            UTType.png.identifier as CFString, 1, nil
        ) else { die("cannot write to \(out)", 3) }
        CGImageDestinationAddImage(dest, image, nil)
        guard CGImageDestinationFinalize(dest) else { die("failed writing \(out)", 3) }

        print("display \(displayIndex) \(display.width)x\(display.height) @\(scale)x -> \(image.width)x\(image.height) -> \(out)")
        status = 0
    } catch {
        die("capture failed: \(error)", 3)
    }
}

semaphore.wait()
exit(status)
