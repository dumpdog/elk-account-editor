using System.IO.Compression;
using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class ZoneSheetTests
{
    private static byte[] Csv(string s) => Encoding.UTF8.GetBytes(s);

    [Fact]
    public void Csv_With_Header_Any_Column_Order()
    {
        var r = ZoneSheet.Read(Csv("Name,Zone,Area,Type,Definition\r\nFront Door,1,Area 1,Normally Closed,Burglar Entry/Exit 1\r\nGarage,2,2,0,03\r\n"), "z.csv");
        Assert.Empty(r.Warnings);
        Assert.Equal(new ZoneSheetRow(1, "Front Door", 1, 1, 1), r.Rows[0]);
        Assert.Equal(new ZoneSheetRow(2, "Garage", 3, 0, 2), r.Rows[1]);
    }

    [Fact]
    public void Names_Only_Sheet_Is_Numbered_From_Zone_1()
    {
        var r = ZoneSheet.Read(Csv("Name\nFront Door\nBack Door\n\nKitchen Window\n"), "z.csv");
        Assert.Equal(new[] { 1, 2, 3 }, r.Rows.Select(x => x.N));
        Assert.All(r.Rows, x => { Assert.Null(x.Definition); Assert.Null(x.Type); Assert.Null(x.Area); });
        Assert.Contains(r.Warnings, w => w.Contains("no Zone column"));
    }

    [Fact]
    public void Headerless_Uses_Paste_Box_Order_And_Tabs()
    {
        var r = ZoneSheet.Read(Csv("5\tPatio\t1\t0\t1\n6\tShed\t8\t2\t3\n"), "z.txt");
        Assert.Equal(new ZoneSheetRow(5, "Patio", 1, 0, 1), r.Rows[0]);
        Assert.Equal(new ZoneSheetRow(6, "Shed", 8, 2, 3), r.Rows[1]);
    }

    [Fact]
    public void Quoted_Cells_Semicolons_And_Bom()
    {
        var bytes = new UTF8Encoding(true).GetPreamble().Concat(Csv("Zone;Name\n1;\"Door, front\"\n2;\"He said \"\"hi\"\"\"\n")).ToArray();
        var r = ZoneSheet.Read(bytes, "z.csv");
        Assert.Equal("Door, front", r.Rows[0].Name);
        Assert.Equal("He said \"hi\"", r.Rows[1].Name);
    }

    [Fact]
    public void Bad_Rows_Are_Skipped_With_A_Warning_And_Long_Names_Cut()
    {
        var r = ZoneSheet.Read(Csv("Zone,Name,Definition\n0,Zero,1\n209,Too high,1\nabc,Text,1\n7,A name that is much too long,Nonsense\n"), "z.csv");
        var only = Assert.Single(r.Rows);
        Assert.Equal(7, only.N); Assert.Equal("A name that is m", only.Name); Assert.Null(only.Definition);
        Assert.Equal(5, r.Warnings.Count);   // three skipped rows, the cut name, the unknown definition
    }

    [Fact]
    public void Duplicate_Zone_Is_Reported()
    {
        var r = ZoneSheet.Read(Csv("Zone,Name\n3,One\n3,Two\n"), "z.csv");
        Assert.Contains(r.Warnings, w => w.Contains("Zone 3 appears 2 times"));
    }

    [Theory]
    [InlineData("1", 1)]
    [InlineData("01", 1)]
    [InlineData("Burglar Entry/Exit 1", 1)]
    [InlineData("burglar  entry/exit 2", 2)]
    [InlineData("03 = Burglar Perimeter Instant", 3)]
    [InlineData("Burglar Perimeter", 3)]
    [InlineData("Fire Alarm", 10)]
    [InlineData("99", null)]
    [InlineData("Burglar", null)]            // ambiguous start
    public void Definition_Lookup(string s, int? want) => Assert.Equal(want, ZoneSheet.Lookup(s, ModelMeta.ZoneDefinitions));

    private static List<ZoneDto> Zones() => new()
    {
        new ZoneDto { N = 1, Name = "Front Door", Definition = 1, Type = 1, Area = 1 },
        new ZoneDto { N = 2, Name = "Unused", Definition = 0 },
        new ZoneDto { N = 3, Name = "Hall, \"main\"", Definition = 4, Type = 0, Area = 2 },
    };

    [Fact]
    public void Export_Skips_Disabled_Unless_Asked()
    {
        Assert.Equal(3, ZoneSheet.Table(Zones(), false).Count);          // header + 2
        Assert.Equal(4, ZoneSheet.Table(Zones(), true).Count);
        Assert.Equal(new[] { "1", "Front Door", "Burglar Entry/Exit 1", "Normally Closed", "Area 1" }, ZoneSheet.Table(Zones(), false)[1]);
    }

    [Fact]
    public void Csv_Export_Reads_Back_Identically()
    {
        var back = ZoneSheet.Read(ZoneSheet.ToCsv(ZoneSheet.Table(Zones(), false)), "z.csv");
        Assert.Empty(back.Warnings);
        Assert.Equal(new ZoneSheetRow(1, "Front Door", 1, 1, 1), back.Rows[0]);
        Assert.Equal(new ZoneSheetRow(3, "Hall, \"main\"", 4, 0, 2), back.Rows[1]);
    }

    [Fact]
    public void Xlsx_Export_Reads_Back_Identically_And_Is_A_Valid_Package()
    {
        var bytes = ZoneSheet.ToXlsx(ZoneSheet.Table(Zones(), true));
        using (var z = new ZipArchive(new MemoryStream(bytes))) Assert.NotNull(z.GetEntry("xl/worksheets/sheet1.xml"));
        var back = ZoneSheet.Read(bytes, "z.xlsx");
        Assert.Empty(back.Warnings);
        Assert.Equal(new[] { 1, 2, 3 }, back.Rows.Select(r => r.N));
        Assert.Equal("Hall, \"main\"", back.Rows[2].Name);
        Assert.Equal(0, back.Rows[1].Definition);
    }

    /// <summary>A workbook laid out the way Excel saves it: shared strings, numbers as plain values, cells that skip a column, a second sheet.</summary>
    [Fact]
    public void Reads_An_Excel_Style_Workbook()
    {
        var ms = new MemoryStream();
        using (var z = new ZipArchive(ms, ZipArchiveMode.Create, true))
        {
            void Add(string p, string x) { using var w = new StreamWriter(z.CreateEntry(p).Open(), new UTF8Encoding(false)); w.Write(x); }
            const string ns = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"";
            Add("xl/workbook.xml", $"<workbook {ns} xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Notes\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Zones\" sheetId=\"2\" r:id=\"rId2\"/></sheets></workbook>");
            Add("xl/_rels/workbook.xml.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Target=\"worksheets/sheet2.xml\"/></Relationships>");
            Add("xl/sharedStrings.xml", $"<sst {ns}><si><t>Zone</t></si><si><t>Name</t></si><si><r><t>Front </t></r><r><t>Door</t></r></si><si><t>Type</t></si><si><t>Normally Open</t></si><si><t>Ignore me</t></si></sst>");
            Add("xl/worksheets/sheet1.xml", $"<worksheet {ns}><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>5</v></c></row></sheetData></worksheet>");
            // columns A (Zone), B (Name), D (Type): column C is empty and missing from the XML
            Add("xl/worksheets/sheet2.xml", $"<worksheet {ns}><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c><c r=\"D1\" t=\"s\"><v>3</v></c></row>" +
                "<row r=\"2\"><c r=\"A2\"><v>12</v></c><c r=\"B2\" t=\"s\"><v>2</v></c><c r=\"D2\" t=\"s\"><v>4</v></c></row></sheetData></worksheet>");
        }
        var r = ZoneSheet.Read(ms.ToArray(), "book.xlsx");
        Assert.Empty(r.Warnings);
        Assert.Equal(new ZoneSheetRow(12, "Front Door", null, 2, null), Assert.Single(r.Rows));
    }

    [Fact]
    public void Old_Xls_And_Empty_Files_Give_Clear_Errors()
    {
        var xls = new byte[] { 0xD0, 0xCF, 0x11, 0xE0, 0, 0, 0, 0 };
        Assert.Contains("Save As", Assert.Throws<InvalidDataException>(() => ZoneSheet.Read(xls, "old.xls")).Message);
        Assert.Contains("no rows", Assert.Single(ZoneSheet.Read(Array.Empty<byte>(), "e.csv").Warnings));
    }
}
