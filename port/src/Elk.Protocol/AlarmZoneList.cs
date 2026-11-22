using System.IO.Compression;
using System.Text;
using System.Xml.Linq;

namespace Elk.Protocol;

/// <summary>Fills the "Alarm Zone List ELK" worksheet (the commissioning checklist, embedded as AlarmZoneTemplate.xlsx) from an account.
/// The template is patched in place, so the logo, styles, check boxes and conditional formatting are kept exactly as supplied.
/// Filled: title (account name), created/modified date, and for every zone the name, definition code (Type) and area.
/// Left blank on purpose because the account does not hold them: wire number, power source, location, expander power, notes, sign-offs.
/// Rows the template uses for the second half of a double contact are hidden (unhide them to use them).</summary>
public static class AlarmZoneList
{
    private static readonly XNamespace Ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    private static readonly XNamespace Xml = XNamespace.Xml;
    private const int TemplateLastZoneRow = 172, TemplateLastRow = 175, TemplateLastZone = 128, MaxZone = 208;

    public static byte[] TemplateBytes()
    {
        using var s = typeof(AlarmZoneList).Assembly.GetManifestResourceStream("alarm-zone-template") ?? throw new InvalidDataException("alarm zone template is missing");
        using var ms = new MemoryStream(); s.CopyTo(ms); return ms.ToArray();
    }

    /// <summary>Builds the .xlsx. <paramref name="title"/> defaults to the account name.</summary>
    public static byte[] Build(AccountModel model, string? title = null, DateTime? now = null)
    {
        var date = (now ?? DateTime.Now).ToString("yyyy-MM-dd");
        var zones = model.Zones.GroupBy(z => z.N).ToDictionary(g => g.Key, g => g.First());
        bool Used(ZoneDto z) => z.Definition != 0 || !string.IsNullOrWhiteSpace(z.Name);
        int maxUsed = zones.Values.Where(Used).Select(z => z.N).DefaultIfEmpty(0).Max();

        using var src = new ZipArchive(new MemoryStream(TemplateBytes()), ZipArchiveMode.Read);
        string sheetPath = "xl/worksheets/sheet1.xml", bookPath = "xl/workbook.xml";
        var sheet = LoadXml(src, sheetPath); var book = LoadXml(src, bookPath);
        var strings = LoadXml(src, "xl/sharedStrings.xml").Root!.Elements(Ns + "si").Select(si => string.Concat(si.Descendants(Ns + "t").Select(t => t.Value))).ToList();

        var root = sheet.Root!; var data = root.Element(Ns + "sheetData")!;
        var rows = data.Elements(Ns + "row").ToList();
        string TextOf(XElement? c) => c == null ? "" : c.Attribute("t")?.Value switch { "s" => strings[int.Parse(c.Element(Ns + "v")!.Value)], "inlineStr" => c.Value, _ => c.Element(Ns + "v")?.Value ?? "" };
        XElement? Cell(XElement row, string col) => row.Elements(Ns + "c").FirstOrDefault(c => (string?)c.Attribute("r") == col + (string)row.Attribute("r")!);

        // header block
        var r1 = rows.First(r => RowNo(r) == 1); SetText(GetCell(r1, "B"), title ?? model.Account);
        SetText(GetCell(rows.First(r => RowNo(r) == 2), "B"), date); SetText(GetCell(rows.First(r => RowNo(r) == 3), "B"), date);

        // the wireless block's header says "Type" twice; the second column is the area (shared string 7)
        var f27 = Cell(rows.First(r => RowNo(r) == 27), "F"); if (f27 != null && TextOf(f27) == "Type") { f27.SetAttributeValue("t", "s"); f27.Element(Ns + "v")!.Value = strings.IndexOf("Area").ToString(); }

        var hidden = new HashSet<int>();
        foreach (var row in rows)
        {
            int n = RowNo(row); var a = Cell(row, "A"); var aText = TextOf(a);
            if (aText.StartsWith("Location:")) SetText(a!, "Location:");
            else if (aText.StartsWith("Expander Powered From:")) SetText(a!, "Expander Powered From:");
            else if (int.TryParse(aText, out int zn) && a!.Attribute("t") == null && n >= 7)
            {
                zones.TryGetValue(zn, out var z); bool used = z != null && Used(z);
                SetText(GetCell(row, "B"), used ? z!.Name.Trim() : "");
                if (used && z!.Definition != 0) { SetText(GetCell(row, "E"), z.Definition.ToString("00")); SetNumber(GetCell(row, "F"), z.Area); }
                else { SetText(GetCell(row, "E"), ""); SetText(GetCell(row, "F"), ""); }
                ClearSample(row);
            }
            else if (n >= 7 && string.IsNullOrEmpty(aText) && Cell(row, "G")?.Attribute("t")?.Value == "b" && !string.IsNullOrWhiteSpace(TextOf(Cell(row, "B"))))
            {   // second half of a double contact in the sample project: blank it and hide it
                SetText(GetCell(row, "B"), ""); ClearSample(row); row.SetAttributeValue("hidden", "1"); hidden.Add(n);
            }
        }

        var merges = root.Element(Ns + "mergeCells")!;
        var breaks = root.Element(Ns + "rowBreaks");
        int lastRow = TemplateLastRow;
        if (maxUsed <= TemplateLastZone)
        {   // the template ends with the labels of an empty expander 6: drop them
            foreach (var row in rows.Where(r => RowNo(r) > TemplateLastZoneRow)) row.Remove();
            foreach (var m in merges.Elements().Where(m => MergeRow((string)m.Attribute("ref")!) > TemplateLastZoneRow).ToList()) m.Remove();
            lastRow = TemplateLastZoneRow;
        }
        else
        {   // zones 129+: expander 6 labels are already there; add its header and as many 16-zone blocks as needed
            var proto = rows.First(r => RowNo(r) == 168);      // a blank zone row of the last populated block
            var header = rows.First(r => RowNo(r) == 156);
            int next = TemplateLastRow + 1, first = 129;
            breaks?.Add(new XElement(Ns + "brk", new XAttribute("id", TemplateLastZoneRow), new XAttribute("max", 12), new XAttribute("man", 1)));
            for (int exp = 6; first <= MaxZone && first <= Math.Max(maxUsed, 129); exp++, first += 16)
            {
                if (exp > 6)
                {   // labels for expanders 7 and up
                    breaks?.Add(new XElement(Ns + "brk", new XAttribute("id", next - 1), new XAttribute("max", 12), new XAttribute("man", 1)));
                    var rowsAdd = new[] { ("Location:", 72, true), ("Expander Powered From:", 40, true), ($"Expander: {exp}", 44, false) };
                    for (int i = 0; i < 3; i++)
                    {
                        var lr = new XElement(Ns + "row", new XAttribute("r", next), new XAttribute("spans", "1:13"));
                        lr.Add(StyledCell("A", next, rowsAdd[i].Item2, rowsAdd[i].Item1), StyledCell("B", next, rowsAdd[i].Item2, null), StyledCell("C", next, 41, null));
                        data.Add(lr);
                        if (rowsAdd[i].Item3) merges.Add(new XElement(Ns + "mergeCell", new XAttribute("ref", $"A{next}:B{next}")));
                        next++;
                    }
                }
                var h = Clone(header, next); data.Add(h); next++;
                for (int k = 0; k < 16 && first + k <= MaxZone; k++)
                {
                    int zn = first + k; var zr = Clone(proto, next);
                    SetNumber(GetCell(zr, "A"), zn);
                    zones.TryGetValue(zn, out var z); bool used = z != null && Used(z);
                    SetText(GetCell(zr, "B"), used ? z!.Name.Trim() : "");
                    if (used && z!.Definition != 0) { SetText(GetCell(zr, "E"), z.Definition.ToString("00")); SetNumber(GetCell(zr, "F"), z.Area); }
                    data.Add(zr); next++;
                }
            }
            lastRow = next - 1;
        }
        merges.SetAttributeValue("count", merges.Elements().Count());
        if (breaks != null) { breaks.SetAttributeValue("count", breaks.Elements().Count()); breaks.SetAttributeValue("manualBreakCount", breaks.Elements().Count()); }
        root.Element(Ns + "dimension")!.SetAttributeValue("ref", $"A1:N{lastRow}");
        // the template remembered the cursor on H29; start at the top
        var sel = root.Descendants(Ns + "selection").FirstOrDefault(); if (sel != null) { sel.SetAttributeValue("activeCell", "A1"); sel.SetAttributeValue("sqref", "A1"); }
        foreach (var dn in book.Root!.Descendants(Ns + "definedName").Where(d => (string?)d.Attribute("name") == "_xlnm.Print_Area"))
            dn.Value = $"'Alarm Zone List ELK'!$A$1:$M${lastRow}";

        using var outMs = new MemoryStream();
        using (var dst = new ZipArchive(outMs, ZipArchiveMode.Create, true))
            foreach (var e in src.Entries)
            {
                var ne = dst.CreateEntry(e.FullName, CompressionLevel.Optimal);
                using var os = ne.Open();
                if (e.FullName == sheetPath) Save(sheet, os); else if (e.FullName == bookPath) Save(book, os);
                else { using var es = e.Open(); es.CopyTo(os); }
            }
        return outMs.ToArray();
    }

    // ---- xml helpers -------------------------------------------------------------------------------------------------
    private static XDocument LoadXml(ZipArchive z, string path) { using var s = z.GetEntry(path)!.Open(); return XDocument.Load(s); }
    private static void Save(XDocument d, Stream s)
    {
        d.Declaration = new XDeclaration("1.0", "UTF-8", "yes");
        using var w = new StreamWriter(s, new UTF8Encoding(false)); d.Save(w, SaveOptions.DisableFormatting);
    }
    private static int RowNo(XElement r) => int.Parse((string)r.Attribute("r")!);
    private static int MergeRow(string rf) => int.Parse(new string(rf.Split(':')[0].Where(char.IsDigit).ToArray()));
    private static int ColIndex(string col) => col.Aggregate(0, (a, ch) => a * 26 + (ch - 'A' + 1));
    private static string ColOf(XElement c) => new string(((string)c.Attribute("r")!).TakeWhile(char.IsLetter).ToArray());

    /// <summary>The cell in a column, created in column order when the row has none.</summary>
    private static XElement GetCell(XElement row, string col)
    {
        var rn = (string)row.Attribute("r")!; var want = ColIndex(col);
        foreach (var c in row.Elements(Ns + "c"))
        {
            int ci = ColIndex(ColOf(c));
            if (ci == want) return c;
            if (ci > want) { var nc = new XElement(Ns + "c", new XAttribute("r", col + rn)); c.AddBeforeSelf(nc); return nc; }
        }
        var last = new XElement(Ns + "c", new XAttribute("r", col + rn)); row.Add(last); return last;
    }
    private static void SetText(XElement c, string text)
    {
        c.Attribute("t")?.Remove(); c.Elements().Where(e => e.Name != Ns + "f").ToList().ForEach(e => e.Remove());
        if (text.Length == 0) return;
        c.SetAttributeValue("t", "inlineStr");
        c.Add(new XElement(Ns + "is", new XElement(Ns + "t", new XAttribute(Xml + "space", "preserve"), text)));
    }
    private static void SetNumber(XElement c, int v)
    {
        c.Attribute("t")?.Remove(); c.Elements().Where(e => e.Name != Ns + "f").ToList().ForEach(e => e.Remove());
        c.Add(new XElement(Ns + "v", v));
    }
    private static XElement StyledCell(string col, int row, int style, string? text)
    {
        var c = new XElement(Ns + "c", new XAttribute("r", col + row), new XAttribute("s", style));
        if (text != null) SetText(c, text); return c;
    }
    /// <summary>Clears the hand-filled columns of a zone row (wire number, power source, checked-by, notes) and unticks both check boxes.</summary>
    private static void ClearSample(XElement row)
    {
        foreach (var col in new[] { "C", "D", "I", "L", "M" }) { var c = row.Elements(Ns + "c").FirstOrDefault(x => ColOf(x) == col); if (c != null) SetText(c, ""); }
        foreach (var col in new[] { "G", "J" })
        {
            var c = row.Elements(Ns + "c").FirstOrDefault(x => ColOf(x) == col);
            if (c != null) { c.SetAttributeValue("t", "b"); c.Elements().ToList().ForEach(e => e.Remove()); c.Add(new XElement(Ns + "v", 0)); }
        }
    }
    /// <summary>Copy of a row moved to a new row number (formulas dropped: shared formula ids do not carry over).</summary>
    private static XElement Clone(XElement row, int n)
    {
        var copy = new XElement(row);
        copy.SetAttributeValue("r", n);
        foreach (var f in copy.Descendants(Ns + "f").ToList()) f.Remove();
        foreach (var c in copy.Elements(Ns + "c")) { c.SetAttributeValue("r", ColOf(c) + n); if ((string?)c.Attribute("t") == "str") { c.Attribute("t")!.Remove(); } c.Elements(Ns + "v").Where(v => v.Value.Length == 0).ToList().ForEach(v => v.Remove()); }
        copy.Attribute("hidden")?.Remove();
        return copy;
    }
}
