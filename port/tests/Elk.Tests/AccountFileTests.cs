using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class AccountFileTests
{
    private static string? SamplePath()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "sample", "ElkRPAcctSample Export"); if (File.Exists(p)) return p; d = d.Parent; }
        return null;
    }

    [Fact]
    public void Real_Sample_Roundtrips_Byte_For_Byte_Through_The_Model()
    {
        var p = SamplePath(); if (p == null) return;               // sample is not part of the repo checkout
        var text = File.ReadAllText(p, Encoding.Latin1);
        var f = AccountFile.Parse(text);
        Assert.Equal(64, f.Zones.Entries.Count); Assert.Equal(8, f.Users.Entries.Count); Assert.Equal(1, f.Areas.Entries.Count);
        Assert.Equal(2, f.Keypads.Entries.Count); Assert.Single(f.Phones.Entries);
        Assert.Equal(text, f.ToText());                              // untouched file writes back identically
        AccountModel.From(f).ApplyTo(f);
        Assert.Equal(text, f.ToText());                              // decode + apply without edits changes nothing
    }

    [Fact]
    public void Model_Edits_Land_In_The_Right_Bytes()
    {
        var p = SamplePath(); if (p == null) return;
        var f = AccountFile.Parse(File.ReadAllText(p, Encoding.Latin1));
        var m = AccountModel.From(f);
        var z = m.Zones.First(x => x.N == 2); z.Name = "Patio Door"; z.Flags = new() { "bypassable", "chime", "silent" }; z.Area = 3; z.Definition = 1;
        m.Zones.Add(new ZoneDto { N = 100, Name = "New", Definition = 3 });
        m.Users.First(u => u.N == 2).Code = "1357";
        m.Areas[0].Exit1 = 45;
        m.Phones[0].Number = "8005551212";
        m.ApplyTo(f);
        var f2 = AccountFile.Parse(f.ToText());
        var m2 = AccountModel.From(f2);
        var z2 = m2.Zones.First(x => x.N == 2);
        Assert.Equal("Patio Door", z2.Name); Assert.Equal(3, z2.Area); Assert.Contains("chime", z2.Flags); Assert.Contains("silent", z2.Flags);
        Assert.Equal(65, m2.Zones.Count); Assert.Equal(100, m2.Zones.First(x => x.N > 48 && x.N < 161).N);
        Assert.Equal("1357", m2.Users.First(u => u.N == 2).Code);
        Assert.Equal(45, m2.Areas[0].Exit1); Assert.Equal("8005551212", m2.Phones[0].Number);
        Assert.Equal(m.ZoneGroups, m2.ZoneGroups);
    }
}

public class WirelessGroupTests
{
    [Theory]
    [InlineData("0,0,0,0,0,0,0,0,0,0,0,0,0", true)]
    [InlineData("0,1,1,0,0,0,0,0,0,0,0,0,0", true)]
    [InlineData("0,1,1,1,1,1,1,1,1,1,0,0,0", true)]     // groups 2-10 = zones 17-160
    [InlineData("0,0,1,0,0,0,0,0,0,0,0,0,0", true)]     // ElkRP export with only group 3 on
    [InlineData("0,1,0,1,0,0,0,0,0,0,0,0,0", true)]     // a hardwired expander uses up group 3
    [InlineData("1,1,0,0,0,0,0,0,0,0,0,0,0", false)]   // group 1 can never be wireless
    [InlineData("0,1,1,1,1,1,1,1,1,1,1,0,0", false)]   // zone 161 and above is never wireless
    public void Wireless_Groups_Are_Within_Zones_17_To_160(string csv, bool ok)
    {
        var g = csv.Split(',').Select(int.Parse).ToArray();
        if (ok) AccountModel.ValidateWirelessGroups(g); else Assert.Throws<FormatException>(() => AccountModel.ValidateWirelessGroups(g));
    }
}
