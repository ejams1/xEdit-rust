{
  Parity fixture (issue #31): `SetRoundMode` and the rounding functions the
  mode reaches.

  `xEdit`'s `xejviScriptAdapterMisc.pas` registers `Math.GetRoundMode`,
  `SetRoundMode`, `GetPrecisionMode` and `SetPrecisionMode`; the two setters
  return the previous mode. This probe prints the mode round trip, the
  `Round`/`Trunc`/`Floor`/`Ceil` results under every mode (the halfway cases
  pin the baseline's rounding of .5, the `-2.7`/`2.1` cases separate
  round-to-nearest from toward-zero and toward-infinity), and the text of a
  division (`1/3`) so that the arithmetic the mode may reach is compared
  too. `GetPrecisionMode` is not touched here (`sys-helpers.pas`).

  The probe ends with `rmNearest` restored, so a later probe's output does
  not depend on this one's (the modes are process state, as upstream's are).
}
unit SysRoundProbe;

var
  gD, gE, gF: Double;

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

procedure Rounds(const aTag: string);
begin
  Log('round', aTag + ' 2.5=' + IntToStr(Round(2.5)) + ' 3.5=' + IntToStr(Round(3.5)) + ' 2.7=' + IntToStr(Round(2.7)) + ' 2.1=' + IntToStr(Round(2.1)));
  Log('round', aTag + ' -2.5=' + IntToStr(Round(-2.5)) + ' -3.5=' + IntToStr(Round(-3.5)) + ' -2.7=' + IntToStr(Round(-2.7)) + ' -2.1=' + IntToStr(Round(-2.1)));
  Log('round', aTag + ' trunc2.7=' + IntToStr(Trunc(2.7)) + ' trunc-2.7=' + IntToStr(Trunc(-2.7)));
  Log('round', aTag + ' floor2.7=' + IntToStr(Floor(2.7)) + ' floor-2.1=' + IntToStr(Floor(-2.1)));
  Log('round', aTag + ' ceil2.2=' + IntToStr(Ceil(2.2)) + ' ceil-2.7=' + IntToStr(Ceil(-2.7)));
  gD := 1.0 / 3.0;
  Log('round', aTag + ' third=' + FloatToStrF(gD, 1, 17, 0));
  { The RTL's float-to-text arithmetic (`Power10`, `Round`, the division
    by ten in `FloatToDecimal`) rounds in the mode too (fix-numerics), so
    every format prints the digits of that mode. }
  gD := 0.1 + 0.2;
  Log('round', aTag + ' 0.1+0.2=' + FloatToStrF(gD, 1, 17, 0));
  Log('round', aTag + ' lit0.1=' + FloatToStrF(0.1, 1, 17, 0) + ' lit0.2=' + FloatToStrF(0.2, 1, 17, 0) + ' lit0.3=' + FloatToStrF(0.3, 1, 17, 0));
  gD := 2.0 / 3.0;
  Log('round', aTag + ' twothirds=' + FloatToStrF(gD, 1, 17, 0) + ' g=' + FloatToStr(gD) + ' f=' + FloatToStrF(gD, 2, 15, 6));
  gD := 1.0E300 / 7.0;
  Log('round', aTag + ' big=' + FloatToStrF(gD, 1, 17, 0) + ' g=' + FloatToStr(gD));
  gD := 1.0E-300 / 7.0;
  Log('round', aTag + ' small=' + FloatToStrF(gD, 1, 17, 0) + ' g=' + FloatToStr(gD));
  gD := 5.0E-324;
  Log('round', aTag + ' denormal=' + FloatToStrF(gD, 1, 17, 0));
  gD := -1.0 / 3.0;
  Log('round', aTag + ' negthird=' + FloatToStrF(gD, 1, 17, 0) + ' f=' + FloatToStrF(gD, 2, 15, 3));
  gD := 123456.789;
  Log('round', aTag + ' fixed=' + FloatToStrF(gD, 2, 15, 2) + ' n=' + FloatToStrF(gD, 3, 15, 2) + ' fmt=' + Format('%.4f|%e|%g', [gD, gD, gD]));
  Log('round', aTag + ' pict=' + FormatFloat('#,##0.000', gD) + ' ' + FormatFloat('0.00E+00', gD) + ' ' + FormatFloat('0.#####', 1.0 / 3.0));
  gD := 0.125;
  Log('round', aTag + ' tie=' + FloatToStrF(gD, 2, 15, 2) + ' ' + FormatFloat('0.00', gD) + ' ' + FloatToStrF(1e22, 1, 17, 0));
  gD := 0.1;
  gE := 0.2;
  gF := gD + gE;
  Log('round', aTag + ' sum<0.3=' + Flag(gF < 0.3) + ' sum=0.3=' + Flag(gF = 0.3) + ' sum>0.3=' + Flag(gF > 0.3));
  gF := StrToFloat('0.1') + StrToFloat('0.2');
  Log('round', aTag + ' strsum<0.3=' + Flag(gF < 0.3) + ' strsum=0.3=' + Flag(gF = 0.3) + ' strsum>0.3=' + Flag(gF > 0.3));
  Log('round', aTag + ' 0.1<1/10=' + Flag(gD < 1.0 / 10.0) + ' 0.1=1/10=' + Flag(gD = 1.0 / 10.0) + ' 0.1>1/10=' + Flag(gD > 1.0 / 10.0));
  gF := 1.5 + 0.25;
  Log('round', aTag + ' 1.5+0.25=' + FloatToStrF(gF, 1, 17, 0));
  Log('round', aTag + ' roundto2.25=' + IntToStr(Round(2.25)) + ' roundto1.35=' + IntToStr(Round(1.35)));
end;

function Initialize: integer;
begin
  Result := 0;
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('nearest');
  Log('mode', 'set-down-prev=' + IntToStr(SetRoundMode(rmDown)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('down');
  Log('mode', 'set-up-prev=' + IntToStr(SetRoundMode(rmUp)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('up');
  Log('mode', 'set-truncate-prev=' + IntToStr(SetRoundMode(rmTruncate)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('truncate');
  Log('mode', 'set-nearest-prev=' + IntToStr(SetRoundMode(rmNearest)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
end;

end.
