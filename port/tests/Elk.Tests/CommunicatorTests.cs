using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class CommunicatorTests
{
    private static string? Sample(string name)
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "sample", name); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }

    [Fact]
    public void Communicator_Export_Roundtrips_And_Decodes()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text);
        Assert.True(f.HasComm);
        Assert.Single(f.AreaRcs.Entries); Assert.Equal(8, f.UserRcs.Entries.Count); Assert.Equal(65, f.ZoneRcs.Entries.Count);
        Assert.Equal(text, f.ToText());
        var m = AccountModel.From(f);
        Assert.All(m.Comm!.Areas[0].Codes, c => Assert.Equal(1, c));          // abort..exit error all on
        Assert.Equal(new[] { 1, 1 }, new[] { m.Comm.Users[0].Close, m.Comm.Users[0].Open });
        Assert.Equal(new[] { 1, 0, 0, 0 }, m.Comm.Zones[0].Codes);                    // zone 1 untouched
        Assert.Equal(15, m.Comm.System.AcFailDelay);
        Assert.All(m.Comm.System.Codes, c => Assert.Equal(1, c));
        Assert.Equal(new[] { 1, 1, 1, 0, 0, 0 }, m.Comm.Keypads[0].Codes);
        m.ApplyTo(f);
        Assert.Equal(text, f.ToText());
    }

    [Fact]
    public void Switching_On_The_Pulses_In_The_Old_Export_Gives_The_Communicator_Export()
    {
        var before = Sample("Sample Zones Import Export"); var after = Sample("Sample Communicator"); if (before == null || after == null) return;
        var f = AccountFile.Parse(before); var m = AccountModel.From(f);
        foreach (var a in m.Comm!.Areas) for (int i = 0; i < 17; i++) a.Codes[i] = 1;
        foreach (var u in m.Comm.Users) { u.Close = 1; u.Open = 1; }
        for (int i = 0; i < 18; i++) m.Comm.System.Codes[i] = 1;
        for (int i = 0; i < 3; i++) m.Comm.Keypads[0].Codes[i] = 1;
        m.ApplyTo(f);
        var got = f.ToText().Replace("\"Sample Import\"", "\"Sample Import F1 Com\"");
        Assert.Equal(after, got);
    }

    [Fact]
    public void Report_Code_Out_Of_Range_Is_Refused()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        m.Comm!.System.Codes[0] = 256;
        Assert.Throws<FormatException>(() => m.ApplyTo(f));
    }
}

public class TelephoneTests
{
    private static string? Sample()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "sample", "Sample Communicator"); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }

    [Fact]
    public void Ip_Format_Allows_One_Attempt_And_Area_Codes_Are_Written()
    {
        var text = Sample(); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        var t = m.Phones[0]; t.Format = 6; t.Attempts = 5; t.AccountCodes[0] = "000000"; t.AccountCodes[1] = "009999";
        m.ApplyTo(f);
        var e = f.Phones.Entries[0].Bytes;
        Assert.Equal(6, e[21] >> 4); Assert.Equal(1, e[22]);
        var back = AccountModel.From(AccountFile.Parse(f.ToText())).Phones[0];
        Assert.Equal("000000", back.AccountCodes[0]); Assert.Equal("009999", back.AccountCodes[1]);
    }

    [Fact]
    public void Format_Above_7_Is_Refused()
    {
        var text = Sample(); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f); m.Phones[0].Format = 8;
        Assert.Throws<FormatException>(() => m.ApplyTo(f));
    }
}
