using System.IO.Compression;
using System.Text;
using System.Xml.Linq;

namespace Elk.Protocol;

/// <summary>One zone row from a spreadsheet. A null field means the sheet had no such column (or the cell was empty), so the zone keeps what it has.</summary>
public sealed record ZoneSheetRow(int N, string? Name, int? Definition, int? Type, int? Area);

public sealed class ZoneSheetResult
{
    public List<ZoneSheetRow> Rows { get; } = new();
    public List<string> Columns { get; } = new();
    public List<string> Warnings { get; } = new();
}

/// <summary>
/// Reads a zone list from an Excel workbook (.xlsx) or a CSV/TSV file and writes one back out. No packages: an .xlsx is a zip of XML, read and written here.
/// Columns are found by their header text (Zone / Number / #, Name / Description, Definition / Function, Type / Wiring, Area / Partition), in any order.
/// With no header row the columns are Zone, Name, Definition, Type, Area, the same as the paste box. A sheet that has a Name column but no Zone column
/// is numbered from zone 1 in row order. Definition and Type may be given as the text ElkRP shows ("Burglar Entry/Exit 1") or as the number.
/// </summary>
public static class ZoneSheet
{
    public const int MaxZone = 208;

    // ---- reading ---------------------------------------------------------------------------------------------------------------------------

    public static ZoneSheetResult Read(byte[] data, string fileName)
    {
        List<List<string>> table;
        bool zip = data.Length > 3 && data[0] == 'P' && data[1] == 'K';
        if (zip) table = ReadXlsx(data);
        else if (fileName.EndsWith(".xls", StringComparison.OrdinalIgnoreCase) && data.Length > 3 && data[0] == 0xD0 && data[1] == 0xCF)
            throw new InvalidDataException("This is an old Excel (.xls) file. Open it in Excel and use Save As > Excel Workbook (.xlsx) or CSV.");
        else table = ReadDelimited(DecodeText(data));
        return Interpret(table);
    }

    private static string DecodeText(byte[] d)
    {
        if (d.Length >= 3 && d[0] == 0xEF && d[1] == 0xBB && d[2] == 0xBF) return Encoding.UTF8.GetString(d, 3, d.Length - 3);
        if (d.Length >= 2 && d[0] == 0xFF && d[1] == 0xFE) return Encoding.Unicode.GetString(d, 2, d.Length - 2);      // Excel "Unicode Text"
        try { return new UTF8Encoding(false, true).GetString(d); } catch (DecoderFallbackException) { return Encoding.Latin1.GetString(d); }
    }

    /// <summary>Splits CSV or tab separated text. The separator is whichever of tab, semicolon or comma the first line has most of (tab wins ties); quotes work as in Excel.</summary>
    public static List<List<string>> ReadDelimited(string text)
    {
        int nl = text.IndexOfAny(new[] { '\r', '\n' });
        var first = nl < 0 ? text : text[..nl];
        char sep = ',';
        int best = first.Count(c => c == ',');
        if (first.Count(c => c == ';') > best) { sep = ';'; best = first.Count(c => c == ';'); }
        if (first.Count(c => c == '\t') >= Math.Max(best, 1)) sep = '\t';
        var rows = new List<List<string>>(); var row = new List<string>(); var sb = new StringBuilder(); bool q = false, any = false;
        void EndCell() { row.Add(sb.ToString()); sb.Clear(); any = true; }
        void EndRow() { if (any || sb.Length > 0) { EndCell(); rows.Add(row); } row = new(); any = false; }
        for (int i = 0; i < text.Length; i++)
        {
            char c = text[i];
            if (q) { if (c == '"') { if (i + 1 < text.Length && text[i + 1] == '"') { sb.Append('"'); i++; } else q = false; } else sb.Append(c); }
            else if (c == '"' && sb.Length == 0) { q = true; any = true; }
            else if (c == sep) EndCell();
            else if (c == '\r' || c == '\n') { if (c == '\r' && i + 1 < text.Length && text[i + 1] == '\n') i++; EndRow(); }
            else sb.Append(c);
        }
        EndRow();
        return rows;
    }

    private static readonly XNamespace Ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    private static readonly XNamespace RelNs = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    private static XDocument Load(ZipArchive z, string path)
    {
        var e = z.GetEntry(path) ?? throw new InvalidDataException("The workbook is missing " + path + ".");
        using var s = e.Open(); return XDocument.Load(s);
    }

    private static List<List<string>> ReadXlsx(byte[] data)
    {
        try
        {
            using var z = new ZipArchive(new MemoryStream(data), ZipArchiveMode.Read);
            var shared = new List<string>();
            if (z.GetEntry("xl/sharedStrings.xml") != null)
                foreach (var si in Load(z, "xl/sharedStrings.xml").Root!.Elements(Ns + "si"))
                    shared.Add(string.Concat(si.Descendants(Ns + "t").Where(t => t.Parent?.Name != Ns + "rPh").Select(t => t.Value)));      // rich text runs joined; phonetic hints skipped
            // the first worksheet, or one called "Zones" if there is
            var wb = Load(z, "xl/workbook.xml").Root!;
            var rels = Load(z, "xl/_rels/workbook.xml.rels").Root!.Elements().ToDictionary(r => (string?)r.Attribute("Id") ?? "", r => (string?)r.Attribute("Target") ?? "");
            var sheets = wb.Element(Ns + "sheets")!.Elements(Ns + "sheet").ToList();
            var pick = sheets.FirstOrDefault(s => string.Equals((string?)s.Attribute("name"), "Zones", StringComparison.OrdinalIgnoreCase)) ?? sheets[0];
            var target = rels[(string)pick.Attribute(RelNs + "id")!];
            var path = target.StartsWith('/') ? target.TrimStart('/') : "xl/" + target;
            var rows = new List<List<string>>();
            foreach (var r in Load(z, path).Root!.Descendants(Ns + "row"))
            {
                var cells = new List<string>(); int nextCol = 0;
                foreach (var c in r.Elements(Ns + "c"))
                {
                    int col = ColumnIndex((string?)c.Attribute("r"), nextCol);
                    while (cells.Count < col) cells.Add("");
                    cells.Add(CellText(c, shared)); nextCol = col + 1;
                }
                if (cells.Any(x => x.Length > 0)) rows.Add(cells);
            }
            return rows;
        }
        catch (Exception e) when (e is InvalidDataException or System.Xml.XmlException or KeyNotFoundException or NullReferenceException or InvalidOperationException)
        { throw new InvalidDataException("Could not read this Excel file: " + e.Message); }
    }

    private static int ColumnIndex(string? cellRef, int fallback)
    {
        if (string.IsNullOrEmpty(cellRef)) return fallback;
        int n = 0, i = 0;
        for (; i < cellRef.Length && char.IsLetter(cellRef[i]); i++) n = n * 26 + (char.ToUpperInvariant(cellRef[i]) - 'A' + 1);
        return i == 0 ? fallback : n - 1;
    }

    private static string CellText(XElement c, List<string> shared)
    {
        var t = (string?)c.Attribute("t");
        if (t == "inlineStr") return string.Concat(c.Descendants(Ns + "t").Select(x => x.Value)).Trim();
        var v = c.Element(Ns + "v")?.Value ?? "";
        if (t == "s") return int.TryParse(v, out var i) && i >= 0 && i < shared.Count ? shared[i].Trim() : "";
        if (t == "b") return v == "1" ? "TRUE" : "FALSE";
        if (t is null or "n" && double.TryParse(v, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var d) && d == Math.Floor(d) && Math.Abs(d) < 1e9)
            return ((long)d).ToString();                                        // 1 not 1.0; Excel stores whole numbers without a point already, this covers 1E+1 style
        return v.Trim();
    }

    // ---- turning a table into zone rows ------------------------------------------------------------------------------------------------------

    private static string Norm(string s) => new string(s.Where(char.IsLetterOrDigit).ToArray()).ToLowerInvariant();

    private static readonly Dictionary<string, string> HeaderNames = new()
    {
        ["zone"] = "n", ["zonenumber"] = "n", ["zoneno"] = "n", ["number"] = "n", ["no"] = "n", ["num"] = "n", ["input"] = "n", ["inputnumber"] = "n", ["zn"] = "n", ["id"] = "n",
        ["name"] = "name", ["zonename"] = "name", ["description"] = "name", ["label"] = "name", ["text"] = "name", ["location"] = "name",
        ["definition"] = "def", ["zonedefinition"] = "def", ["function"] = "def", ["zonefunction"] = "def", ["zonetype"] = "def", ["alarmtype"] = "def", ["alarmdefinition"] = "def",
        ["type"] = "type", ["wiring"] = "type", ["zonewiring"] = "type", ["wiretype"] = "type", ["circuit"] = "type", ["circuittype"] = "type",
        ["area"] = "area", ["partition"] = "area", ["areanumber"] = "area",
    };

    private static ZoneSheetResult Interpret(List<List<string>> table)
    {
        var res = new ZoneSheetResult();
        if (table.Count == 0) { res.Warnings.Add("The file has no rows."); return res; }
        // the header is the first row where at least one cell is a known column name and none is a bare zone number
        var cols = new Dictionary<string, int>(); int start = 0;
        var head = table[0];
        var named = head.Select((h, i) => (h: Norm(h), i)).Where(x => HeaderNames.ContainsKey(x.h)).ToList();
        if (named.Count > 0 && !head.Any(h => int.TryParse(h.Trim(), out _)))
        {
            foreach (var (h, i) in named) { var k = HeaderNames[h]; if (!cols.ContainsKey(k)) cols[k] = i; }
            start = 1;
        }
        else { cols["n"] = 0; cols["name"] = 1; cols["def"] = 2; cols["type"] = 3; cols["area"] = 4; }
        if (!cols.ContainsKey("n") && !cols.ContainsKey("name")) throw new InvalidDataException("No Zone or Name column found. Put headers such as Zone, Name, Definition, Type, Area in the first row.");
        bool sequential = !cols.ContainsKey("n");
        if (sequential) res.Warnings.Add("There is no Zone column, so the names are applied to zones 1, 2, 3 … in row order.");
        foreach (var k in new[] { "n", "name", "def", "type", "area" }) if (cols.ContainsKey(k)) res.Columns.Add(k switch { "n" => "Zone", "name" => "Name", "def" => "Definition", "type" => "Type", _ => "Area" });

        string Cell(List<string> r, string k) => cols.TryGetValue(k, out var i) && i < r.Count ? r[i].Trim() : "";
        int seq = 0;
        for (int line = start; line < table.Count; line++)
        {
            var r = table[line]; string where = $"row {line + 1}";
            if (r.All(c => c.Trim().Length == 0)) continue;
            if (r.Count > 0 && r[0].TrimStart().StartsWith('#')) continue;
            seq++;
            int n;
            if (sequential) n = seq;
            else
            {
                var zs = Cell(r, "n"); if (zs.Length == 0) { res.Warnings.Add($"{where}: no zone number, skipped."); continue; }
                var m = System.Text.RegularExpressions.Regex.Match(zs, @"^(?:zone\s*)?0*(\d{1,3})$", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
                if (!m.Success) { res.Warnings.Add($"{where}: \"{zs}\" is not a zone number, skipped."); continue; }
                n = int.Parse(m.Groups[1].Value);
            }
            if (n < 1 || n > MaxZone) { res.Warnings.Add($"{where}: zone {n} is outside 1-{MaxZone}, skipped."); continue; }
            string? name = null; int? def = null, type = null, area = null;
            if (cols.ContainsKey("name")) { var s = Cell(r, "name"); if (s.Length > 0) { if (s.Length > 16) { res.Warnings.Add($"{where}: zone {n} name \"{s}\" is longer than 16 characters and was cut to \"{s[..16]}\"."); s = s[..16]; } name = s; } }
            if (cols.ContainsKey("def")) { var s = Cell(r, "def"); if (s.Length > 0) { def = Lookup(s, ModelMeta.ZoneDefinitions); if (def == null) res.Warnings.Add($"{where}: zone {n} definition \"{s}\" is not recognised, left unchanged."); } }
            if (cols.ContainsKey("type")) { var s = Cell(r, "type"); if (s.Length > 0) { type = Lookup(s, ModelMeta.ZoneTypes); if (type == null) res.Warnings.Add($"{where}: zone {n} type \"{s}\" is not recognised, left unchanged."); } }
            if (cols.ContainsKey("area")) { var s = Cell(r, "area"); if (s.Length > 0) { var m = System.Text.RegularExpressions.Regex.Match(s, @"(\d+)"); if (m.Success && int.Parse(m.Value) is >= 1 and <= 8) area = int.Parse(m.Value); else res.Warnings.Add($"{where}: zone {n} area \"{s}\" is not 1-8, left unchanged."); } }
            res.Rows.Add(new ZoneSheetRow(n, name, def, type, area));
        }
        // a zone listed twice: the last row wins, and the user is told
        foreach (var g in res.Rows.GroupBy(r => r.N).Where(g => g.Count() > 1)) res.Warnings.Add($"Zone {g.Key} appears {g.Count()} times; the last row is used.");
        return res;
    }

    /// <summary>Finds a definition or type by its number ("3", "03") or its text, ignoring case, spaces and punctuation; accepts "03 = Burglar Perimeter Instant" and "Burglar Perimeter Instant".</summary>
    public static int? Lookup(string s, string[] names)
    {
        s = s.Trim();
        var m = System.Text.RegularExpressions.Regex.Match(s, @"^(\d{1,2})\s*(?:[=:\-–]\s*(.*))?$");
        int? number = null; string key = Norm(s);
        if (m.Success)
        {
            int v = int.Parse(m.Groups[1].Value);
            if (v < names.Length) number = v;
            key = Norm(m.Groups[2].Value);                              // "03 = Burglar…": the text decides when there is any
            if (key.Length == 0) return number;
        }
        for (int i = 0; i < names.Length; i++) if (Norm(names[i]) == key) return i;
        if (key.Length >= 4)                                            // an unambiguous start such as "Burglar Perimeter"
        {
            var hits = Enumerable.Range(0, names.Length).Where(i => Norm(names[i]).StartsWith(key)).ToList();
            if (hits.Count == 1) return hits[0];
        }
        return number;
    }

    // ---- writing -----------------------------------------------------------------------------------------------------------------------------

    private static readonly string[] Head = { "Zone", "Name", "Definition", "Type", "Area" };

    /// <summary>The rows to export: every zone that is not Disabled, or all of them.</summary>
    public static List<string[]> Table(IEnumerable<ZoneDto> zones, bool includeDisabled)
    {
        var t = new List<string[]> { Head };
        foreach (var z in zones.Where(z => includeDisabled || z.Definition != 0).OrderBy(z => z.N))
            t.Add(new[] { z.N.ToString(), z.Name, Pick(ModelMeta.ZoneDefinitions, z.Definition), Pick(ModelMeta.ZoneTypes, z.Type), "Area " + z.Area });
        return t;
    }
    private static string Pick(string[] a, int i) => i >= 0 && i < a.Length ? a[i] : i.ToString();

    public static byte[] ToCsv(List<string[]> table)
    {
        var sb = new StringBuilder("﻿");                                   // the byte order mark makes Excel read it as UTF-8
        foreach (var r in table) sb.Append(string.Join(",", r.Select(Quote))).Append("\r\n");
        return new UTF8Encoding(false).GetBytes(sb.ToString());
    }
    private static string Quote(string s) => s.IndexOfAny(new[] { ',', '"', '\r', '\n' }) >= 0 || s != s.Trim() ? "\"" + s.Replace("\"", "\"\"") + "\"" : s;

    public static byte[] ToXlsx(List<string[]> table)
    {
        var ms = new MemoryStream();
        using (var z = new ZipArchive(ms, ZipArchiveMode.Create, true))
        {
            void Add(string path, string xml) { var e = z.CreateEntry(path, CompressionLevel.Optimal); using var w = new StreamWriter(e.Open(), new UTF8Encoding(false)); w.Write(xml); }
            const string decl = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
            Add("[Content_Types].xml", decl + "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>" +
                "<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>" +
                "<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>");
            Add("_rels/.rels", decl + "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>");
            Add("xl/workbook.xml", decl + "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Zones\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>");
            Add("xl/_rels/workbook.xml.rels", decl + "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>");
            Add("xl/styles.xml", decl + "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>" +
                "<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/></cellXfs></styleSheet>");
            var sb = new StringBuilder(decl + "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>" +
                "<cols><col min=\"1\" max=\"1\" width=\"7\" customWidth=\"1\"/><col min=\"2\" max=\"2\" width=\"20\" customWidth=\"1\"/><col min=\"3\" max=\"3\" width=\"30\" customWidth=\"1\"/><col min=\"4\" max=\"4\" width=\"26\" customWidth=\"1\"/><col min=\"5\" max=\"5\" width=\"9\" customWidth=\"1\"/></cols><sheetData>");
            for (int r = 0; r < table.Count; r++)
            {
                sb.Append($"<row r=\"{r + 1}\">");
                for (int c = 0; c < table[r].Length; c++)
                {
                    string at = (char)('A' + c) + (r + 1).ToString(), v = table[r][c];
                    if (r > 0 && c == 0 && int.TryParse(v, out var num)) sb.Append($"<c r=\"{at}\"><v>{num}</v></c>");
                    else sb.Append($"<c r=\"{at}\" t=\"inlineStr\"{(r == 0 ? " s=\"1\"" : "")}><is><t xml:space=\"preserve\">{System.Security.SecurityElement.Escape(v)}</t></is></c>");
                }
                sb.Append("</row>");
            }
            sb.Append("</sheetData></worksheet>");
            Add("xl/worksheets/sheet1.xml", sb.ToString());
        }
        return ms.ToArray();
    }
}
