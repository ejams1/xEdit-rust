{
  Parity fixture (issue #31): the system, locale and disk helpers of
  `JvInterpreter_SysUtils` and `xejviScriptAdapterMisc` -- `DiskFree`/
  `DiskSize`, `GetLocaleStr`/`GetLocaleChar`, the FPU precision mode and
  `Beep`, plus the RTL's default format settings the locale stands on.

  `disk`: the free space of a volume is machine state (it changes between
  two runs, like a key's state), so only the facts that do not change are
  printed: `DiskSize` of every fixed volume (the volume size does not change
  at runtime, and the number is the JvInterpreter wrapper's `Integer` of the
  RTL's `Int64`, so it is truncated), the -1 sentinel of the drives that do
  not exist (B: and Z:), the current-drive form (drive argument 0) against
  the lettered form, and whether a real volume answers the -1 sentinel. The
  absolute `DiskFree` value is not printed. The drive argument is a `Byte`:
  the two conversions a script can hit (a value above 255 and a negative
  one) are printed with their exception.

  `locale`: `GetLocaleStr` over the whole 0..127 type range for the system
  locale (0) and selected types for en-US (1033), and `GetLocaleChar` for
  the separator, sign and digit types. A value that holds a NUL (the font
  signature) is printed as its length alone: the GUI's memo hangs on a
  NUL message (`x-105`), and the length is what a comparison can hold.

  `fpu`: `GetPrecisionMode`/`SetPrecisionMode` (the x87 control word) and
  `GetRoundMode` without setting it (the modes' effect on `Round` is
  `sys-round.pas`).

  `beep`: `Beep` has no script-visible result (it plays the system sound);
  the probe pins that it returns and the run continues.

  `fmt`: the default format settings a `DateToStr`/`TimeToStr`/`Format`
  call reads, which the RTL settles from the same locale data.
}
unit SysHelpersProbe;

function Flag(aValue: Boolean): string;
begin
  if aValue then
    Result := 'T'
  else
    Result := 'F';
end;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure DiskOne(const aTag: string; aDrive: Integer);
var
  free, size: Integer;
begin
  free := DiskFree(aDrive);
  size := DiskSize(aDrive);
  Log('disk', aTag + ' free-is-minus1=' + Flag(free = -1) + ' size-is-minus1=' + Flag(size = -1) + ' size=' + IntToStr(size));
end;

procedure DiskErrors(const aTag: string; aDrive: Integer);
begin
  try
    Log('disk', aTag + '=' + IntToStr(DiskFree(aDrive)));
  except on E: Exception do
    Log('disk', aTag + '-exc=' + E.ClassName + ' ' + E.Message);
  end;
  try
    Log('disk', aTag + '-size=' + IntToStr(DiskSize(aDrive)));
  except on E: Exception do
    Log('disk', aTag + '-size-exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

procedure Disk;
begin
  Log('disk', 'size0=' + IntToStr(DiskSize(0)) + ' size3=' + IntToStr(DiskSize(3)));
  Log('disk', 'size4=' + IntToStr(DiskSize(4)) + ' size5=' + IntToStr(DiskSize(5)) + ' size13=' + IntToStr(DiskSize(13)));
  DiskOne('0', 0);
  DiskOne('2', 2);
  DiskOne('3', 3);
  DiskOne('26', 26);
  DiskErrors('300', 300);
  DiskErrors('neg', -1);
end;

function SafeLocale(aLocale, aType: Integer): string;
var
  s: string;
begin
  s := GetLocaleStr(aLocale, aType, 'DEF');
  if Pos(#0, s) > 0 then
    Result := 'NUL len=' + IntToStr(Length(s))
  else
    Result := s;
end;

procedure Locale;
var
  i: Integer;
begin
  for i := 0 to 127 do
    Log('locale', IntToStr(i) + '=' + SafeLocale(0, i));
  for i := 256 to 267 do
    Log('locale', IntToStr(i) + '=' + SafeLocale(0, i));
  Log('locale', '1000=' + SafeLocale(0, $1000));
  Log('locale', '1001=' + SafeLocale(0, $1001));
  Log('locale', '1002=' + SafeLocale(0, $1002));
  Log('locale', '1003=' + SafeLocale(0, $1003));
  Log('locale', '1004=' + SafeLocale(0, $1004));
  Log('locale', '1005=' + SafeLocale(0, $1005));
  Log('locale', '1006=' + SafeLocale(0, $1006));
  Log('locale', '1007=' + SafeLocale(0, $1007));
  Log('locale', '1008=' + SafeLocale(0, $1008));
  Log('locale', '1011=' + SafeLocale(0, $1011));
  Log('locale', '1012=' + SafeLocale(0, $1012));
  Log('locale', '1013=' + SafeLocale(0, $1013));
  Log('locale', '1014=' + SafeLocale(0, $1014));
  Log('locale', '9999=' + SafeLocale(0, 9999));
  Log('enus', '1=' + SafeLocale(1033, 1) + ' 2=' + SafeLocale(1033, 2));
  Log('enus', '11=' + SafeLocale(1033, 11) + ' 14=' + SafeLocale(1033, 14) + ' 15=' + SafeLocale(1033, 15));
  Log('enus', '20=' + SafeLocale(1033, 20) + ' 21=' + SafeLocale(1033, 21));
  Log('enus', '29=' + SafeLocale(1033, 29) + ' 30=' + SafeLocale(1033, 30));
  Log('enus', '31=' + SafeLocale(1033, 31) + ' 32=' + SafeLocale(1033, 32));
  Log('enus', '40=' + SafeLocale(1033, 40) + ' 41=' + SafeLocale(1033, 41));
  Log('enus', '44=' + SafeLocale(1033, 44) + ' 56=' + SafeLocale(1033, 56));
  Log('enus', '80=' + SafeLocale(1033, 80) + ' 81=' + SafeLocale(1033, 81) + ' 84=' + SafeLocale(1033, 84));
  Log('enus', '89=' + SafeLocale(1033, 89) + ' 90=' + SafeLocale(1033, 90));
  Log('enus', '4098=' + SafeLocale(1033, $1002) + ' 4099=' + SafeLocale(1033, $1003));
  Log('enus', '9999=' + SafeLocale(9999, 14));
  Log('char', '14=' + GetLocaleChar(0, 14, 'D'));
  Log('char', '15=' + GetLocaleChar(0, 15, 'D'));
  Log('char', '20=' + GetLocaleChar(0, 20, 'D'));
  Log('char', '29=' + GetLocaleChar(0, 29, 'D'));
  Log('char', '30=' + GetLocaleChar(0, 30, 'D'));
  Log('char', '40=' + GetLocaleChar(0, 40, 'D'));
  Log('char', '44=' + GetLocaleChar(0, 44, 'D'));
  Log('char', '56=' + GetLocaleChar(0, 56, 'D'));
  Log('char', '80=' + GetLocaleChar(0, 80, 'D') + ' 81=' + GetLocaleChar(0, 81, 'D'));
  Log('char', '200=' + GetLocaleChar(0, 200, 'D'));
end;

procedure Fpu;
begin
  Log('fpu', 'prec-get=' + IntToStr(GetPrecisionMode()));
  Log('fpu', 'prec-set-single=' + IntToStr(SetPrecisionMode(pmSingle)));
  Log('fpu', 'prec-get=' + IntToStr(GetPrecisionMode()));
  Log('fpu', 'prec-set-extended=' + IntToStr(SetPrecisionMode(pmExtended)));
  Log('fpu', 'prec-get=' + IntToStr(GetPrecisionMode()));
  Log('fpu', 'prec-set-reserved=' + IntToStr(SetPrecisionMode(pmReserved)));
  Log('fpu', 'prec-get=' + IntToStr(GetPrecisionMode()));
  Log('fpu', 'prec-set-double=' + IntToStr(SetPrecisionMode(pmDouble)));
  Log('fpu', 'prec-get=' + IntToStr(GetPrecisionMode()));
  Log('fpu', 'round-get=' + IntToStr(GetRoundMode()));
end;

procedure BeepOnce;
begin
  Beep;
  Log('beep', 'after');
end;

procedure FormatSettings;
begin
  Log('fmt', 'datetostr=' + DateToStr(EncodeDate(2026, 10, 16)));
  Log('fmt', 'timetostr=' + TimeToStr(EncodeTime(15, 7, 42, 0)));
  Log('fmt', 'datetimetostr=' + DateTimeToStr(EncodeDate(2026, 10, 16) + EncodeTime(15, 7, 42, 0)));
end;

function Initialize: integer;
begin
  Result := 0;
  Disk;
  Locale;
  Fpu;
  BeepOnce;
  FormatSettings;
end;

end.
