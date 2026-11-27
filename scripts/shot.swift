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
        region = CGRect(x: parts[0], y: parts[1], width: parts[2], height: parts[3])
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
        guard let display = content.displays.first else { die("no display found", 3) }

        var excluded: [SCWindow] = []
        if hideOverlay {
            excluded = content.windows.filter { $0.title == "overlay" }
        }

        let filter = SCContentFilter(display: display, excludingWindows: excluded)
        let config = SCStreamConfiguration()
        // The display's width is in points; double it to keep the native 2x pixels.
        config.width = display.width * 2
        config.height = display.height * 2
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

        print("\(image.width)x\(image.height) -> \(out)")
        status = 0
    } catch {
        die("capture failed: \(error)", 3)
    }
}

semaphore.wait()
exit(status)
