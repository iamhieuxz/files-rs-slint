// Sinh icon ứng dụng (.ico nhiều kích thước) từ một SVG đơn giản.
//
// File .ico là container chứa nhiều ảnh PNG ở các kích thước khác nhau; Windows
// chọn kích thước phù hợp cho title bar, taskbar, alt-tab và Explorer. Thiếu
// kích thước nào thì Windows tự thu nhỏ và icon bị nhòe.

using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;

class MakeIcon
{
    static void Main(string[] args)
    {
        string outPath = args.Length > 0 ? args[0] : "app.ico";
        int[] sizes = { 16, 20, 24, 32, 40, 48, 64, 128, 256 };
        var images = new Bitmap[sizes.Length];

        for (int i = 0; i < sizes.Length; i++)
            images[i] = Draw(sizes[i]);

        using (var fs = new FileStream(outPath, FileMode.Create))
        using (var w = new BinaryWriter(fs))
        {
            // ICONDIR
            w.Write((short)0);            // reserved
            w.Write((short)1);            // type = icon
            w.Write((short)sizes.Length); // count

            // Ghi bảng mục lục trước, offset tính sau.
            var pngs = new byte[sizes.Length][];
            for (int i = 0; i < sizes.Length; i++)
            {
                using (var ms = new MemoryStream())
                {
                    images[i].Save(ms, ImageFormat.Png);
                    pngs[i] = ms.ToArray();
                }
            }

            int offset = 6 + 16 * sizes.Length;
            for (int i = 0; i < sizes.Length; i++)
            {
                int s = sizes[i];
                // 256 được mã hoá là 0 trong ICONDIRENTRY.
                w.Write((byte)(s >= 256 ? 0 : s));
                w.Write((byte)(s >= 256 ? 0 : s));
                w.Write((byte)0);   // palette
                w.Write((byte)0);   // reserved
                w.Write((short)1);  // planes
                w.Write((short)32); // bpp
                w.Write(pngs[i].Length);
                w.Write(offset);
                offset += pngs[i].Length;
            }
            foreach (var png in pngs) w.Write(png);
        }

        Console.WriteLine("wrote " + outPath + " with " + sizes.Length + " sizes");
    }

    // Logo: thư mục Fluent trên nền gradient xanh, bo góc — cùng ngôn ngữ hình
    // với icon trong app.
    static Bitmap Draw(int size)
    {
        var bmp = new Bitmap(size, size, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.InterpolationMode = InterpolationMode.HighQualityBicubic;
            g.Clear(Color.Transparent);

            float pad = size * 0.06f;
            var rect = new RectangleF(pad, pad, size - pad * 2, size - pad * 2);
            float radius = size * 0.22f;

            using (var path = RoundedRect(rect, radius))
            using (var brush = new LinearGradientBrush(
                rect,
                Color.FromArgb(255, 61, 132, 245),
                Color.FromArgb(255, 26, 86, 190),
                45f))
            {
                g.FillPath(brush, path);
            }

            // Glyph thư mục, vẽ theo tỉ lệ để nét luôn đúng ở mọi kích thước.
            float u = size / 24f;
            using (var glyph = new GraphicsPath())
            {
                // Thân thư mục
                glyph.AddBezier(3.5f * u, 7.0f * u, 3.5f * u, 6.2f * u, 4.2f * u, 5.5f * u, 5.0f * u, 5.5f * u);
                glyph.AddLine(9.0f * u, 5.5f * u, 11.0f * u, 8.0f * u);
                glyph.AddLine(11.0f * u, 8.0f * u, 19.0f * u, 8.0f * u);
                glyph.AddBezier(19.0f * u, 8.0f * u, 19.8f * u, 8.0f * u, 20.5f * u, 8.7f * u, 20.5f * u, 9.5f * u);
                glyph.AddLine(20.5f * u, 9.5f * u, 20.5f * u, 17.5f * u);
                glyph.AddBezier(20.5f * u, 17.5f * u, 20.5f * u, 18.3f * u, 19.8f * u, 19.0f * u, 19.0f * u, 19.0f * u);
                glyph.AddLine(19.0f * u, 19.0f * u, 5.0f * u, 19.0f * u);
                glyph.AddBezier(5.0f * u, 19.0f * u, 4.2f * u, 19.0f * u, 3.5f * u, 18.3f * u, 3.5f * u, 17.5f * u);
                glyph.CloseFigure();

                using (var white = new SolidBrush(Color.FromArgb(245, 255, 255, 255)))
                    g.FillPath(white, glyph);
            }
        }
        return bmp;
    }

    static GraphicsPath RoundedRect(RectangleF r, float radius)
    {
        var p = new GraphicsPath();
        float d = radius * 2;
        p.AddArc(r.X, r.Y, d, d, 180, 90);
        p.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        p.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
        p.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        p.CloseFigure();
        return p;
    }
}
