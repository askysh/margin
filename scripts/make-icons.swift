// The one source for Margin's brand images. It draws every icon and logo file from the
// same geometry, so they always match.
//
//   swift scripts/make-icons.swift            (from the repo root; or: bun run icons)
//
// It writes:
//   src-tauri/icons/  app-icon.png (1024), icon.png (512), 32x32.png, 128x128.png,
//                     128x128@2x.png, icon.icns (all 10 sizes, through iconutil),
//                     tray-template.png and tray-recording.png (menu bar, 36 px = 18 pt @2x)
//   docs/images/      app-icon-light.png, app-icon-dark.png (256), social-preview.png (1280x640)
//   src/components/logoMark.ts   the mark as SVG path data, for <Logo />
//   src-tauri/icons/icon.ico     the Windows program icon (16 to 256 px)
//   windows/msix/Assets/         the MSIX package logos (docs/windows-store.md)
//   windows/store/               the Microsoft Store listing images
//
// The mark: a sound line in the margin crosses the green margin rule and turns into
// handwriting on the page. The recording goes in the margin; your words go on the page.
import AppKit
import CoreText

let root = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."
let iconsDir = "\(root)/src-tauri/icons"
let docsDir = "\(root)/docs/images"
let srgb = CGColorSpace(name: CGColorSpace.sRGB)!

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

// MARK: - Palette (from src/styles.css)

struct Palette {
    let paperTop, paperBottom, ruling, rule, ink: CGColor
    let rim: CGFloat
}
let light = Palette(paperTop: rgb(0xf7f9f3), paperBottom: rgb(0xe1e8db), ruling: rgb(0xc6d2c3),
                    rule: rgb(0x4e8a73), ink: rgb(0x1c2430), rim: 0.9)
let dark = Palette(paperTop: rgb(0x29323a), paperBottom: rgb(0x161b20), ruling: rgb(0xffffff, 0.07),
                   rule: rgb(0x6fa893), ink: rgb(0xe3e8e1), rim: 0.32)

// MARK: - Geometry (1024 design units, top-left origin)

/// The macOS 26 icon grid: an 824 body centred on the 1024 canvas.
let body = CGRect(x: 100, y: 100, width: 824, height: 824)
let bodyRadius: CGFloat = 185.4

/// Apple's continuous-corner rounded rect.
func squircle(_ r: CGRect, _ radius: CGFloat) -> CGPath {
    let p = CGMutablePath()
    let k = min(radius, min(r.width, r.height) / 2 / 1.52866483)
    func tl(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: r.minX + x * k, y: r.minY + y * k) }
    func tr(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: r.maxX - x * k, y: r.minY + y * k) }
    func br(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: r.maxX - x * k, y: r.maxY - y * k) }
    func bl(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: r.minX + x * k, y: r.maxY - y * k) }
    func inward(_ f: (CGFloat, CGFloat) -> CGPoint) {
        p.addCurve(to: f(0.66993427, 0.06549600), control1: f(1.08849323, 0), control2: f(0.86840689, 0))
        p.addLine(to: f(0.63149399, 0.07491100))
        p.addCurve(to: f(0.07491176, 0.63149399), control1: f(0.37282392, 0.16905899), control2: f(0.16906013, 0.37282401))
        p.addCurve(to: f(0, 1.52866483), control1: f(0, 0.86840701), control2: f(0, 1.08849299))
    }
    func outward(_ f: (CGFloat, CGFloat) -> CGPoint) {
        p.addCurve(to: f(0.07491176, 0.63149399), control1: f(0, 1.08849299), control2: f(0, 0.86840701))
        p.addCurve(to: f(0.63149399, 0.07491100), control1: f(0.16906013, 0.37282401), control2: f(0.37282392, 0.16905899))
        p.addLine(to: f(0.66993427, 0.06549600))
        p.addCurve(to: f(1.52866483, 0), control1: f(0.86840689, 0), control2: f(1.08849323, 0))
    }
    p.move(to: tl(1.52866483, 0))
    p.addLine(to: tr(1.52866483, 0)); inward(tr)
    p.addLine(to: br(0, 1.52866483)); outward(br)
    p.addLine(to: bl(1.52866483, 0)); inward(bl)
    p.addLine(to: tl(0, 1.52866483)); outward(tl)
    p.closeSubpath()
    return p
}

/// How much detail the mark keeps. Each size gets the most it can show cleanly.
enum Level {
    case full    // 128 px and up: a rising three-crest waveform, three loops, the ruling
    case small   // 64 px: one and a half waves, two loops
    case tiny    // 32 px and the menu bar: one wave, one loop, heavy lines
    case micro   // 16 px: one wave and a straight line

    static func forIcon(_ px: Int) -> Level { px <= 16 ? .micro : px <= 32 ? .tiny : px <= 64 ? .small : .full }
}

/// The mark: the line runs along the baseline, crossing the margin rule at ruleX.
struct Mark {
    let level: Level
    /// Path pieces: straight runs of sampled points, and cubic curves.
    enum Seg { case to(CGPoint); case curve(CGPoint, CGPoint, CGPoint) }

    struct Params { let x0, periods, amp, loops, r, d, k, decay, stroke, ruleWidth: CGFloat }
    var baseline: CGFloat { 560 }
    /// 16 px moves the rule right so the wave gets room enough to show.
    var ruleX: CGFloat { level == .micro ? 470 : 392 }
    var stroke: CGFloat { [.full: 26, .small: 58, .tiny: 70, .micro: 84][level]! }
    var ruleWidth: CGFloat { [.full: 26, .small: 44, .tiny: 50, .micro: 64][level]! }
    var showsRuling: Bool { level == .full }

    func segments() -> [Seg] {
        let base = baseline
        func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: x, y: y) }
        // Small sizes are drawn by hand on a 32 px grid (32 units a pixel): one 3 px wave of
        // sound in the margin, a short flat run past the rule, then, in `tiny`, one upright
        // loop whose counter stays about 2 px wide; `micro` goes straight on instead.
        let wave: [Seg] = [
            .to(pt(170, base)),
            .curve(pt(192, base - 200), pt(248, base - 200), pt(270, base)),
            .curve(pt(292, base + 200), pt(348, base + 200), pt(370, base)),
        ]
        switch level {
        case .micro:
            return [
                .to(pt(160, base)),
                .curve(pt(190, base - 250), pt(285, base - 250), pt(315, base)),
                .curve(pt(345, base + 250), pt(440, base + 250), pt(470, base)),
                .curve(pt(600, base), pt(740, base - 10), pt(860, base - 24)),
            ]
        case .tiny:
            return wave + [
                .curve(pt(400, base), pt(440, base), pt(470, base)),
                .curve(pt(560, base), pt(640, base - 120), pt(640, base - 250)),
                .curve(pt(640, base - 365), pt(500, base - 365), pt(500, base - 250)),
                .curve(pt(500, base - 110), pt(510, base + 6), pt(590, base + 4)),
                .curve(pt(680, base + 2), pt(770, base - 10), pt(850, base - 30)),
            ]
        default: break
        }
        if level == .full { return fullLine() }
        let q = Params(x0: 175, periods: 1.5, amp: 150, loops: 2, r: 27, d: 50, k: 100, decay: 0.55, stroke: 58, ruleWidth: 44)
        var segs: [Seg] = []
        // Sound: a sine under a swelling envelope, from the page edge to the rule.
        for i in 0...300 {
            let f = CGFloat(i) / 300
            let env = q.amp * pow(sin(.pi * f), 0.9)
            segs.append(.to(pt(q.x0 + f * (ruleX - q.x0), base - env * sin(2 * .pi * q.periods * f))))
        }
        // Pen: a prolate cycloid, loops like a cursive "l", getting smaller as it goes.
        var e = CGPoint.zero
        for i in 1...600 {
            let f = CGFloat(i) / 600
            let t = CGFloat.pi + f * q.loops * 2 * .pi
            let decay = 1 - f * q.decay
            e = pt(ruleX + q.r * (t - .pi) - q.d * decay * sin(t), base - q.k * decay * (1 + cos(t)))
            segs.append(.to(e))
        }
        segs.append(.curve(pt(e.x + 40, base + 2), pt(e.x + 70, base - 12), pt(e.x + 100, base - 24)))
        return segs
    }

    /// The full mark as one pen movement: a trochoid whose phase t runs from -π/2 to 11π.
    /// Up to 4π it is a waveform, a sine centred on the baseline with crests at 0, 2π and 4π
    /// rising like a spoken word toward the rule. Over 4π…5π it blends into cursive loops that
    /// stand on the baseline (tops at 6π, 8π, 10π) and shrink as they go, so the last crest
    /// comes down past the rule, turns on the baseline and goes straight up into the first loop.
    func fullLine() -> [Seg] {
        let base = baseline, pi = CGFloat.pi
        func smooth(_ a: CGFloat, _ b: CGFloat, _ t: CGFloat) -> CGFloat {
            let u = min(max((t - a) / (b - a), 0), 1); return u * u * (3 - 2 * u)
        }
        let t0 = -pi / 2, handoff = 5 * pi, t1 = 11 * pi, n = 1400
        let dt = (t1 - t0) / CGFloat(n)
        var pts: [CGPoint] = [], x: CGFloat = 0
        for i in 0...n {
            let t = t0 + CGFloat(i) * dt
            let pen = smooth(4 * pi, handoff, t)               // 0 = sound, 1 = pen
            // Units a radian: 84 a wave, 126 a loop, and a burst at the hand-off so the turn
            // on the baseline is a round pen turn, not a corner.
            let turn = 30 * exp(-pow((t - handoff) / 0.55, 2))
            if i > 0 { x += (13.4 + 6.6 * pen + turn) * dt }
            let shrink = 1 - 0.55 * max(0, (t - handoff) / (t1 - handoff))
            let amp = 70 + 105 * max(0, t) / (4 * pi)          // crests 70, 122, 175
            let loop = 92 * shrink
            let sweep = smooth(4.6 * pi, 6 * pi, t)            // the loops' back-stroke, eased in
            pts.append(CGPoint(x: x - 50 * shrink * sweep * sin(t),
                               y: base - (amp * cos(t) * (1 - pen) + loop * (1 + cos(t)) * pen)))
        }
        // Put the hand-off, where the line first sits on the baseline, just past the rule.
        let dx = ruleX + 28 - pts[Int(((handoff - t0) / dt).rounded())].x
        var segs: [Seg] = pts.map { .to(CGPoint(x: $0.x + dx, y: $0.y)) }
        let e = CGPoint(x: pts[n].x + dx, y: pts[n].y)
        segs.append(.curve(CGPoint(x: e.x + 28, y: base + 2), CGPoint(x: e.x + 50, y: base - 9), CGPoint(x: e.x + 72, y: base - 20)))
        return segs
    }

    var path: CGPath {
        let p = CGMutablePath()
        for (i, s) in segments().enumerated() {
            switch s {
            case .to(let q): i == 0 ? p.move(to: q) : p.addLine(to: q)
            case .curve(let c1, let c2, let q): p.addCurve(to: q, control1: c1, control2: c2)
            }
        }
        return p
    }

    var strokedBounds: CGRect {
        path.copy(strokingWithWidth: stroke, lineCap: .round, lineJoin: .round, miterLimit: 10).boundingBoxOfPath
    }

    /// SVG path data for the line, with the sampled runs thinned to every fourth point.
    var svg: String {
        func f(_ v: CGFloat) -> String { String(format: "%.1f", v) }
        let segs = segments()
        var out = ""
        for (i, s) in segs.enumerated() {
            switch s {
            case .to(let q):
                let last = i == segs.count - 1
                // Keep the first point, every fourth, and any point a curve starts from.
                var keep = i % 4 == 0 || last
                if !last, case .curve = segs[i + 1] { keep = true }
                if i == 0 { out += "M\(f(q.x)) \(f(q.y))" } else if keep { out += "L\(f(q.x)) \(f(q.y))" }
            case .curve(let c1, let c2, let q):
                out += "C\(f(c1.x)) \(f(c1.y)) \(f(c2.x)) \(f(c2.y)) \(f(q.x)) \(f(q.y))"
            }
        }
        return out
    }
}

// MARK: - Drawing

func fillCapsule(_ c: CGContext, _ r: CGRect, _ color: CGColor) {
    let rad = min(r.width, r.height) / 2
    c.addPath(CGPath(roundedRect: r, cornerWidth: rad, cornerHeight: rad, transform: nil))
    c.setFillColor(color); c.fillPath()
}

func strokeLine(_ c: CGContext, _ m: Mark, _ color: CGColor, widen: CGFloat = 0) {
    c.addPath(m.path); c.setStrokeColor(color); c.setLineWidth(m.stroke + widen)
    c.setLineCap(.round); c.setLineJoin(.round); c.strokePath()
}

/// The app icon: shadow, paper body, ruling, the margin rule, the line, a glassy rim.
func drawIcon(_ c: CGContext, _ pal: Palette, level: Level) {
    let m = Mark(level: level)
    let shape = squircle(body, bodyRadius)
    // Shadows ignore the transform: their offset and blur are in device pixels, y up.
    let px = abs(c.ctm.a)
    c.saveGState()
    c.setShadow(offset: CGSize(width: 0, height: -12 * px), blur: 28 * px, color: rgb(0x000000, 0.3))
    c.addPath(shape); c.setFillColor(pal.paperBottom); c.fillPath()
    c.restoreGState()

    c.saveGState()
    c.addPath(shape); c.clip()
    let paper = CGGradient(colorsSpace: srgb, colors: [pal.paperTop, pal.paperBottom] as CFArray, locations: [0, 1])!
    c.drawLinearGradient(paper, start: CGPoint(x: 0, y: 100), end: CGPoint(x: 0, y: 924), options: [])
    if m.showsRuling {
        c.setFillColor(pal.ruling)
        for i in 0..<7 { c.fill(CGRect(x: 100, y: 257.5 + CGFloat(i) * 100, width: 824, height: 5)) }
    }
    fillCapsule(c, CGRect(x: m.ruleX - m.ruleWidth / 2, y: 60, width: m.ruleWidth, height: 1000), pal.rule)
    // Ink: a hairline contact shadow only, 1.5 px at 1024.
    c.setShadow(offset: CGSize(width: 0, height: -1.5 * px), blur: 1.5 * px, color: rgb(0x000000, 0.10))
    strokeLine(c, m, pal.ink)
    c.restoreGState()

    // Rim: bright along the top edge, gone by the middle, a little dark at the bottom.
    c.saveGState()
    c.addPath(shape); c.clip()
    c.addPath(squircle(body.insetBy(dx: 1.5, dy: 1.5), bodyRadius - 1.5)); c.setLineWidth(3)
    c.replacePathWithStrokedPath(); c.clip()
    let rim = CGGradient(colorsSpace: srgb, colors: [rgb(0xffffff, pal.rim), rgb(0xffffff, 0), rgb(0x000000, 0.12)] as CFArray,
                         locations: [0, 0.5, 1])!
    c.drawLinearGradient(rim, start: CGPoint(x: 0, y: 100), end: CGPoint(x: 0, y: 924), options: [])
    c.restoreGState()
}

/// Menu-bar template: black on transparent; macOS tints it. Drawn on a 36 px canvas (18 pt @2x)
/// with line widths set in pixels. A gap where the line crosses the rule keeps the two apart.
/// `recording` swaps the sound wave for a solid dot on the rule, and the loop for a straight line.
/// Both share one frame, so the rule stays put when a recording starts.
func drawTray(_ c: CGContext, recording: Bool) {
    let m = Mark(level: .tiny)
    let b = m.strokedBounds
    c.saveGState()
    c.scaleBy(x: 1024 / 36, y: 1024 / 36)            // now in pixels
    let s = 32 / b.width                              // pixels per design unit
    c.translateBy(x: 18, y: 18); c.scaleBy(x: s, y: s); c.translateBy(x: -b.midX, y: -b.midY)
    let px = 1 / s, line = 3.4 * px, gap = 1.6 * px
    let black = rgb(0x000000)
    fillCapsule(c, CGRect(x: m.ruleX - 1.5 * px, y: m.baseline - 15 * px, width: 3 * px, height: 30 * px), black)
    func stroke(_ path: CGPath, _ w: CGFloat) {
        c.addPath(path); c.setLineWidth(w); c.setLineCap(.round); c.setLineJoin(.round); c.setStrokeColor(black); c.strokePath()
    }
    if recording {
        let dot = CGRect(x: m.ruleX - 6 * px, y: m.baseline - 6 * px, width: 12 * px, height: 12 * px)
        c.setBlendMode(.clear)
        c.addEllipse(in: dot.insetBy(dx: -gap, dy: -gap)); c.setFillColor(black); c.fillPath()
        c.setBlendMode(.normal)
        c.addEllipse(in: dot); c.setFillColor(black); c.fillPath()
        let pen = CGMutablePath()
        pen.move(to: CGPoint(x: dot.maxX + gap + line / 2 + 1.5 * px, y: m.baseline))
        pen.addCurve(to: CGPoint(x: b.maxX - m.stroke / 2, y: m.baseline - 30), control1: CGPoint(x: dot.maxX + 200, y: m.baseline),
                     control2: CGPoint(x: b.maxX - 200, y: m.baseline - 12))
        stroke(pen, line)
    } else {
        c.setBlendMode(.clear); stroke(m.path, line + 2 * gap)
        c.setBlendMode(.normal); stroke(m.path, line)
    }
    c.restoreGState()
}

/// Draws in 1024 units onto a canvas of `w`x`h` px, top-left origin.
func render(_ w: Int, _ h: Int, _ path: String, unit: CGFloat = 1024, _ draw: (CGContext) -> Void) {
    let c = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: srgb,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    c.translateBy(x: 0, y: CGFloat(h))
    c.scaleBy(x: CGFloat(min(w, h)) / unit, y: -CGFloat(min(w, h)) / unit)
    draw(c)
    let png = NSBitmapImageRep(cgImage: c.makeImage()!).representation(using: .png, properties: [:])!
    try! png.write(to: URL(fileURLWithPath: path))
}

func icon(_ px: Int, _ path: String, _ pal: Palette = light) {
    render(px, px, path) { drawIcon($0, pal, level: .forIcon(px)) }
}

// MARK: - Windows and the Microsoft Store

/// Windows draws no plate behind an icon and gives it less room than macOS does. So the body
/// fills 90% of the canvas here, not the 80% of the macOS grid.
func winIcon(_ px: Int, _ path: String) {
    render(px, px, path) { c in
        let k: CGFloat = 0.90 * 1024 / body.width
        c.translateBy(x: 512, y: 512); c.scaleBy(x: k, y: k); c.translateBy(x: -512, y: -512)
        drawIcon(c, light, level: .forIcon(px))
    }
}

/// An .ico file is a small directory followed by one PNG for each size. A size of 256 is
/// written as 0, because the field is one byte.
func ico(_ sizes: [Int], _ path: String) {
    let pngs = sizes.map { px -> Data in
        let tmp = NSTemporaryDirectory() + "margin-ico-\(getpid())-\(px).png"
        winIcon(px, tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        return try! Data(contentsOf: URL(fileURLWithPath: tmp))
    }
    var out = Data()
    func le16(_ v: Int) { out.append(UInt8(v & 0xff)); out.append(UInt8((v >> 8) & 0xff)) }
    func le32(_ v: Int) { le16(v & 0xffff); le16((v >> 16) & 0xffff) }
    le16(0); le16(1); le16(sizes.count)
    var offset = 6 + 16 * sizes.count
    for (px, png) in zip(sizes, pngs) {
        out.append(UInt8(px % 256)); out.append(UInt8(px % 256)); out.append(0); out.append(0)
        le16(1); le16(32); le32(png.count); le32(offset)
        offset += png.count
    }
    pngs.forEach { out.append($0) }
    try! out.write(to: URL(fileURLWithPath: path))
}

/// Store art: the macOS-shaped icon in the middle of a sheet of the app's paper.
/// `w` and `h` are in pixels. The icon takes `share` of the shorter side.
func storeArt(_ w: Int, _ h: Int, _ path: String, share: CGFloat) {
    render(w, h, path) { c in
        let W: CGFloat = 1024 * CGFloat(w) / CGFloat(min(w, h)), H: CGFloat = 1024 * CGFloat(h) / CGFloat(min(w, h))
        let paper = CGGradient(colorsSpace: srgb, colors: [light.paperTop, light.paperBottom] as CFArray, locations: [0, 1])!
        c.drawLinearGradient(paper, start: .zero, end: CGPoint(x: 0, y: H), options: [])
        let k = share * 1024 / body.width
        c.translateBy(x: W / 2, y: H / 2); c.scaleBy(x: k, y: k); c.translateBy(x: -512, y: -512)
        drawIcon(c, light, level: .full)
    }
}

// MARK: - Social preview

func recursive(size: CGFloat, weight: CGFloat, casual: CGFloat) -> CTFont {
    let url = URL(fileURLWithPath: "\(root)/node_modules/@fontsource-variable/recursive/files/recursive-latin-full-normal.woff2")
    guard let descs = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as? [CTFontDescriptor], let d = descs.first else {
        fatalError("Recursive not found at \(url.path). Run bun install first.")
    }
    func tag(_ s: String) -> Int { s.unicodeScalars.reduce(0) { $0 << 8 | Int($1.value) } }
    let axes = [tag("wght"): weight, tag("CASL"): casual, tag("MONO"): 0, tag("slnt"): 0, tag("CRSV"): 0.5] as [Int: CGFloat]
    let v = CTFontDescriptorCreateCopyWithAttributes(d, [kCTFontVariationAttribute: axes] as CFDictionary)
    return CTFontCreateWithFontDescriptor(v, size, nil)
}

func text(_ c: CGContext, _ s: String, _ font: CTFont, _ color: CGColor, x: CGFloat, baseline: CGFloat, tracking: CGFloat = 0) {
    let attrs: [NSAttributedString.Key: Any] = [.init(kCTFontAttributeName as String): font,
                                                .init(kCTForegroundColorAttributeName as String): color,
                                                .init(kCTKernAttributeName as String): tracking]
    let line = CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: attrs))
    c.saveGState()
    c.textMatrix = .identity
    c.translateBy(x: x, y: baseline); c.scaleBy(x: 1, y: -1)
    c.textPosition = .zero
    CTLineDraw(line, c)
    c.restoreGState()
}

/// 1280x640: a ruled page, the icon in the margin, the name and the tagline on the lines.
func drawSocial(_ c: CGContext) {
    let W: CGFloat = 1280, H: CGFloat = 640
    let paper = CGGradient(colorsSpace: srgb, colors: [rgb(0xf3f6ef), rgb(0xe6ece1)] as CFArray, locations: [0, 1])!
    c.drawLinearGradient(paper, start: .zero, end: CGPoint(x: 0, y: H), options: [])
    // Ruling on the page side only, so the margin is a clean column for the icon.
    let marginX: CGFloat = 476
    c.setFillColor(rgb(0xd3dcd0))
    var y: CGFloat = 12
    while y < H { c.fill(CGRect(x: marginX, y: y, width: W - marginX, height: 2)); y += 56 }
    c.setFillColor(rgb(0x5e9480, 0.55)); c.fill(CGRect(x: marginX, y: 0, width: 3, height: H))
    // Icon, 360 px, centred in the margin.
    c.saveGState()
    c.translateBy(x: 238 - 180, y: H / 2 - 180); c.scaleBy(x: 360 / 1024, y: 360 / 1024)
    drawIcon(c, light, level: .full)
    c.restoreGState()
    // Name and tagline sit on the ruling (lines at 12 + 56n).
    let tagline = NSFont.systemFont(ofSize: 38, weight: .regular) as CTFont
    text(c, "Margin", recursive(size: 150, weight: 680, casual: 1), rgb(0x1c2430), x: 534, baseline: 292, tracking: -3)
    text(c, "Notes in your words.", tagline, rgb(0x445060), x: 540, baseline: 404)
    text(c, "Transcripts on the side.", tagline, rgb(0x445060), x: 540, baseline: 460)
}

// MARK: - Output

try! FileManager.default.createDirectory(atPath: docsDir, withIntermediateDirectories: true)

icon(1024, "\(iconsDir)/app-icon.png")
icon(512, "\(iconsDir)/icon.png")
icon(32, "\(iconsDir)/32x32.png")
icon(128, "\(iconsDir)/128x128.png")
icon(256, "\(iconsDir)/128x128@2x.png")

let iconset = NSTemporaryDirectory() + "margin-\(getpid()).iconset"
try! FileManager.default.createDirectory(atPath: iconset, withIntermediateDirectories: true)
for pt in [16, 32, 128, 256, 512] {
    icon(pt, "\(iconset)/icon_\(pt)x\(pt).png")
    icon(pt * 2, "\(iconset)/icon_\(pt)x\(pt)@2x.png")
}
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset, "-o", "\(iconsDir)/icon.icns"]
try! iconutil.run(); iconutil.waitUntilExit()
precondition(iconutil.terminationStatus == 0, "iconutil failed")
try? FileManager.default.removeItem(atPath: iconset)

render(36, 36, "\(iconsDir)/tray-template.png") { drawTray($0, recording: false) }
render(36, 36, "\(iconsDir)/tray-recording.png") { drawTray($0, recording: true) }

icon(256, "\(docsDir)/app-icon-light.png", light)
icon(256, "\(docsDir)/app-icon-dark.png", dark)
render(1280, 640, "\(docsDir)/social-preview.png", unit: 640, drawSocial)

// Windows: the program icon, the MSIX package logos and the Store listing images. The file
// names in Assets are the ones Windows looks for (docs/windows-store.md).
let assetsDir = "\(root)/windows/msix/Assets", storeDir = "\(root)/windows/store"
for dir in [assetsDir, storeDir] { try! FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true) }
ico([16, 24, 32, 48, 64, 256], "\(iconsDir)/icon.ico")
winIcon(44, "\(assetsDir)/Square44x44Logo.png")
winIcon(88, "\(assetsDir)/Square44x44Logo.scale-200.png")
for px in [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256] {
    winIcon(px, "\(assetsDir)/Square44x44Logo.targetsize-\(px).png")
    winIcon(px, "\(assetsDir)/Square44x44Logo.targetsize-\(px)_altform-unplated.png")
}
winIcon(150, "\(assetsDir)/Square150x150Logo.png")
winIcon(300, "\(assetsDir)/Square150x150Logo.scale-200.png")
winIcon(50, "\(assetsDir)/StoreLogo.png")
winIcon(100, "\(assetsDir)/StoreLogo.scale-200.png")
winIcon(300, "\(storeDir)/app-tile-300.png")
storeArt(2160, 2160, "\(storeDir)/box-art-2160.png", share: 0.56)
storeArt(1440, 2160, "\(storeDir)/poster-1440x2160.png", share: 0.62)

// The bare mark for the UI: the icon without its tile, drawn straight on the app's paper.
func markSource(_ m: Mark) -> String {
    // A square 790 units wide around the line; the rule runs its full height, as in the icon.
    let b = m.strokedBounds
    let box = CGRect(x: 510 - 395, y: b.midY - 395, width: 790, height: 790)
    let rule = CGRect(x: m.ruleX - m.ruleWidth / 2, y: box.minY, width: m.ruleWidth, height: box.height)
    func f(_ v: CGFloat) -> String { String(format: "%.1f", v) }
    return """
      {
        viewBox: "\(f(box.minX)) \(f(box.minY)) \(f(box.width)) \(f(box.height))",
        rule: { x: \(f(rule.minX)), y: \(f(rule.minY)), width: \(f(rule.width)), height: \(f(rule.height)) },
        stroke: \(f(m.stroke)),
        line: "\(m.svg)",
      }
    """
}
let ts = """
// Generated by scripts/make-icons.swift. Do not edit: change the script and run `bun run icons`.
// The Margin mark in the app icon's 1024 units: the margin rule and the sound-to-pen line.

export type MarkGeometry = {
  viewBox: string;
  rule: { x: number; y: number; width: number; height: number };
  stroke: number;
  line: string;
};

/** For 40 px and up. */
export const MARK: MarkGeometry =
\(markSource(Mark(level: .full)));

/** Under 40 px: one wave, one loop, heavier lines. */
export const MARK_TINY: MarkGeometry =
\(markSource(Mark(level: .tiny)));

"""
try! ts.write(toFile: "\(root)/src/components/logoMark.ts", atomically: true, encoding: .utf8)
print("Wrote icons to \(iconsDir), images to \(docsDir), and src/components/logoMark.ts")
