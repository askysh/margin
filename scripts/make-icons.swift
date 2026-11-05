// Draws the Margin app icon (1024px) and the menu-bar template icon (36px @2x).
// Usage: swift scripts/make-icons.swift <out-dir>
import AppKit

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."

func png(_ size: CGFloat, _ path: String, _ draw: (CGRect) -> Void) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    draw(CGRect(x: 0, y: 0, width: size, height: size))
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
}

// App icon: warm paper squircle, a terracotta margin rule, three ink lines.
png(1024, "\(out)/app-icon.png") { r in
    let inset = r.insetBy(dx: 100, dy: 100)
    let shape = NSBezierPath(roundedRect: inset, xRadius: 185, yRadius: 185)
    NSGradient(starting: NSColor(calibratedRed: 0.99, green: 0.97, blue: 0.93, alpha: 1),
               ending: NSColor(calibratedRed: 0.95, green: 0.91, blue: 0.84, alpha: 1))!
        .draw(in: shape, angle: -90)
    NSColor(calibratedWhite: 0, alpha: 0.08).setStroke(); shape.lineWidth = 4; shape.stroke()
    let rule = NSBezierPath(roundedRect: CGRect(x: 330, y: 230, width: 26, height: 564), xRadius: 13, yRadius: 13)
    NSColor(calibratedRed: 0.82, green: 0.38, blue: 0.27, alpha: 1).setFill(); rule.fill()
    NSColor(calibratedRed: 0.17, green: 0.16, blue: 0.15, alpha: 1).setFill()
    for (i, w) in [300.0, 250.0, 180.0].enumerated() {
        NSBezierPath(roundedRect: CGRect(x: 410, y: 640 - Double(i) * 130, width: w, height: 44),
                     xRadius: 22, yRadius: 22).fill()
    }
}

// Tray template icon: black glyph on transparent (macOS tints it).
png(36, "\(out)/tray-template.png") { _ in
    NSColor.black.setFill()
    NSBezierPath(roundedRect: CGRect(x: 7, y: 6, width: 4, height: 24), xRadius: 2, yRadius: 2).fill()
    for (i, w) in [17.0, 14.0, 10.0].enumerated() {
        NSBezierPath(roundedRect: CGRect(x: 15, y: 24 - Double(i) * 8, width: w, height: 4), xRadius: 2, yRadius: 2).fill()
    }
}
// Recording variant: filled dot.
png(36, "\(out)/tray-recording.png") { _ in
    NSColor.black.setFill()
    NSBezierPath(ovalIn: CGRect(x: 8, y: 8, width: 20, height: 20)).fill()
}
