using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

/// <summary>Software 5.3.44 account with no telephones (count line is "0"). The old shape search latched onto the area report-code block instead.</summary>
public class NoPhoneAccountTests
{
    private static string? Testing(string name)
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "TESTING", name); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }

    [Fact]
    public void NoTelephones_Account_Finds_Communicator_Block()
    {
        var text = Testing("RHTEST"); if (text == null) return;
        var f = AccountFile.Parse(text);
        Assert.Empty(f.Phones.Entries);
        Assert.True(f.HasComm);
        Assert.Single(f.AreaRcs.Entries); Assert.Equal(4, f.UserRcs.Entries.Count); Assert.Equal(16, f.ZoneRcs.Entries.Count);
        Assert.Equal(text, f.ToText());
    }

    [Fact]
    public void NoTelephones_Account_Communicator_Edit_Roundtrips()
    {
        var text = Testing("RHTEST"); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        m.Comm!.Areas[0].Codes[2] = 3; m.Comm.System.Codes[1] = 3;
        m.ApplyTo(f);
        var m2 = AccountModel.From(AccountFile.Parse(f.ToText()));
        Assert.Equal(3, m2.Comm!.Areas[0].Codes[2]); Assert.Equal(3, m2.Comm.System.Codes[1]);
        Assert.Equal(m.Zones.Count, m2.Zones.Count); Assert.Empty(m2.Phones);
    }

    [Fact]
    public void NoTelephones_Account_Can_Add_A_Telephone()
    {
        var text = Testing("RHTEST"); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        Assert.True(m.HasPhones); Assert.Empty(m.Phones);
        m.Phones.Add(new PhoneDto { N = 1, Name = "Central", Number = "5551234", Format = 6, Attempts = 3, AccountCodes = new[] { "009999", "000000", "000000", "000000", "000000", "000000", "000000", "000000" }, Flags = new() { "areaEvents", "globalEvents" } });
        m.Comm!.Areas[0].Codes[0] = 1;
        m.ApplyTo(f);
        var text2 = f.ToText();
        Assert.Equal(text.Split('\n').Length + 89, text2.Split('\n').Length);      // one phone = 1 id line + 88 bytes
        var m2 = AccountModel.From(AccountFile.Parse(text2));
        var p = Assert.Single(m2.Phones);
        Assert.Equal("Central", p.Name); Assert.Equal("5551234", p.Number); Assert.Equal(6, p.Format); Assert.Equal(1, p.Attempts);
        Assert.Equal("009999", p.AccountCodes[0]); Assert.Equal("000000", p.AccountCodes[1]);
        Assert.Equal(new[] { "areaEvents", "globalEvents" }, p.Flags.OrderBy(x => x).ToArray());
        Assert.Equal(1, m2.Comm!.Areas[0].Codes[0]); Assert.Equal(4, m2.Comm.Users.Count); Assert.Equal(16, m2.Comm.Zones.Count);   // communicator block still found after the shift
        Assert.Equal(m.Zones.Count, m2.Zones.Count);
    }
}
