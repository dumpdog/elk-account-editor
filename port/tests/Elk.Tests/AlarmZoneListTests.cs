using System.IO.Compression;
using System.Text;
using System.Xml.Linq;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class AlarmZoneListTests
{
    private static readonly XNamespace Ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

    private static AccountModel Model(params (int n, string name, int def, int area)[] zones)
    {
        var m = new AccountModel { Account = "Test Site" };
        foreach (var z in zones) m.Zones.Add(new ZoneDto { N = z.n, Name = z.name, Definition = z.def, Area = z.area });
        return m;
    }
    private static Dictionary<string, XElement> Cells(byte[] xlsx, out XDocument sheet, out XDocument book)
    {
        using var zip = new ZipArchive(new MemoryStream(xlsx));
        using (var s = zip.GetEntry("xl/worksheets/sheet1.xml")!.Open()) sheet = XDocument.Load(s);
        using (var s = zip.GetEntry("xl/workbook.xml")!.Open()) book = XDocument.Load(s);
        return sheet.Descendants(Ns + "c").ToDictionary(c => (string)c.Attribute("r")!);
    }
    private static string Text(Dictionary<string, XElement> c, string r) => !c.TryGetValue(r, out var e) ? "" : e.Attribute("t")?.Value == "inlineStr" ? e.Value : e.Element(Ns + "v")?.Value ?? "";
    private static int RowOfZone(Dictionary<string, XElement> c, int zone) =>
        c.Where(kv => kv.Key[0] == 'A' && kv.Value.Attribute("t") == null && kv.Value.Element(Ns + "v")?.Value == zone.ToString() && int.Parse(kv.Key[1..]) >= 7).Select(kv => int.Parse(kv.Key[1..])).First();

    [Fact]
    public void Fills_Name_Definition_And_Area_And_Clears_The_Sample_Project()
    {
        var c = Cells(AlarmZoneList.Build(Model((1, "Smoke Detectors", 10, 1), (2, "Sump", 0, 1), (4, "Rec Room Water", 25, 2), (33, "Foyer Motion", 4, 1)), now: new DateTime(2026, 11, 19)), out var sheet, out _);
        Assert.Equal("Test Site", Text(c, "B1")); Assert.Equal("2026-11-19", Text(c, "B2"));
        Assert.Equal("Smoke Detectors", Text(c, "B7")); Assert.Equal("10", Text(c, "E7")); Assert.Equal("1", Text(c, "F7"));
        Assert.Equal("Sump", Text(c, "B8")); Assert.Equal("", Text(c, "E8")); Assert.Equal("", Text(c, "F8"));   // named but disabled: no type or area
        Assert.Equal("25", Text(c, "E10")); Assert.Equal("2", Text(c, "F10")); Assert.Equal("", Text(c, "D10"));  // power source was sample data
        Assert.Equal("", Text(c, "B11")); Assert.Equal("", Text(c, "C11"));                                       // zone 5 not in the account
        int r33 = RowOfZone(c, 33); Assert.Equal("Foyer Motion", Text(c, "B" + r33)); Assert.Equal("04", Text(c, "E" + r33));
        Assert.Equal("7", Text(c, "F27"));                                                                         // header fixed: shared string 7 = Area
        Assert.DoesNotContain("Location: SMC", sheet.ToString());
        Assert.Equal("Location:", Text(c, "A44").Length == 0 ? "Location:" : Text(c, "A44"));
        var r = sheet.Descendants(Ns + "row").First(x => (string)x.Attribute("r")! == "63"); Assert.Equal("1", (string?)r.Attribute("hidden")); Assert.Equal("", Text(c, "B63"));
    }

    [Fact]
    public void Template_Is_Kept_Intact_And_Print_Area_Follows_The_Last_Row()
    {
        var bytes = AlarmZoneList.Build(Model((1, "A", 10, 1)));
        using var src = new ZipArchive(new MemoryStream(AlarmZoneList.TemplateBytes())); using var dst = new ZipArchive(new MemoryStream(bytes));
        Assert.Equal(src.Entries.Select(e => e.FullName).OrderBy(x => x), dst.Entries.Select(e => e.FullName).OrderBy(x => x));
        Cells(bytes, out var sheet, out var book);
        Assert.Contains("$A$1:$M$172", book.ToString());
        Assert.Equal("A1:N172", (string)sheet.Root!.Element(Ns + "dimension")!.Attribute("ref")!);
        Assert.DoesNotContain(sheet.Descendants(Ns + "row"), r => int.Parse((string)r.Attribute("r")!) > 172);
        Assert.All(sheet.Descendants(Ns + "mergeCell"), m => Assert.True(int.Parse(new string(((string)m.Attribute("ref")!).Split(':')[0].Where(char.IsDigit).ToArray())) <= 172));
    }

    [Fact]
    public void Zones_Above_128_Add_Expander_Blocks()
    {
        var c = Cells(AlarmZoneList.Build(Model((130, "Shed Door", 3, 1), (150, "Barn Motion", 4, 1), (208, "Last", 3, 1))), out var sheet, out var book);
        Assert.Equal("Shed Door", Text(c, "B" + RowOfZone(c, 130))); Assert.Equal("03", Text(c, "E" + RowOfZone(c, 130)));
        Assert.Equal("Barn Motion", Text(c, "B" + RowOfZone(c, 150))); Assert.Equal("Last", Text(c, "B" + RowOfZone(c, 208)));
        var nums = sheet.Descendants(Ns + "row").Select(r => int.Parse((string)r.Attribute("r")!)).ToList();
        Assert.Equal(nums.OrderBy(x => x), nums);
        Assert.Equal(nums.Count, nums.Distinct().Count());
        var brk = sheet.Descendants(Ns + "brk").Select(b => int.Parse((string)b.Attribute("id")!)).ToList(); Assert.Equal(brk.OrderBy(x => x), brk);
        Assert.Contains($"$M${nums.Max()}", book.ToString());
        foreach (var row in sheet.Descendants(Ns + "row"))
        {
            var cols = row.Elements(Ns + "c").Select(x => new string(((string)x.Attribute("r")!).TakeWhile(char.IsLetter).ToArray())).Select(s => s.Aggregate(0, (a, ch) => a * 26 + ch - 'A' + 1)).ToList();
            Assert.Equal(cols.OrderBy(x => x), cols);
        }
    }

    [Fact]
    public void Builds_From_A_Real_Export_When_Samples_Exist()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory); string? p = null;
        while (d != null && p == null) { var f = Path.Combine(d.FullName, "sample", "ElkRPAcctSample Export"); if (File.Exists(f)) p = f; d = d.Parent; }
        if (p == null) return;
        var model = AccountModel.From(AccountFile.Parse(File.ReadAllText(p, Encoding.Latin1)));
        var c = Cells(AlarmZoneList.Build(model), out _, out _);
        foreach (var z in model.Zones.Where(z => z.N <= 128 && z.Definition != 0)) Assert.Equal(z.Definition.ToString("00"), Text(c, "E" + RowOfZone(c, z.N)));
    }
}
