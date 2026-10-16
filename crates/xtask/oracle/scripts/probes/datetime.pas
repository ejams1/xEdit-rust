{
  Parity fixture (issue #30): the JSON date-time surface.

  TJsonDataValue.DateTimeValue of every value kind, the ISO 8601 text
  DateTimeToJSON writes with UseUtcTime on (the system's time zone bias and
  the Z suffix) and off (DateTimeToISO8601 with the local offset), the
  parsing surface of JSONToDateTime (a Z suffix, an offset, no suffix, a
  date alone, the .NET "/Date(...)/" form and invalid input) with and
  without the local-time conversion, and the daylight saving cases: a winter
  and a summer value, the missing hour of the spring transition and the
  ambiguous hour of the autumn one. The message log is compared.
}
unit DateTimeProbe;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure WriteValue(const aTag: string; aValue: Double);
var
  obj: TJsonObject;
begin
  obj := TJsonObject.Create;
  obj.D['d'] := aValue;
  Log(aTag, 'utc=' + obj.S['d'] + ' types=' + IntToStr(obj.Types['d']) + ' raw=' + FloatToStr(obj.D['d']));
  SetJDOUseUtcTime(False);
  Log(aTag, 'local=' + obj.S['d']);
  SetJDOUseUtcTime(True);
  obj.Free;
end;

procedure ParseValue(const aTag, aText: string);
var
  obj: TJsonObject;
begin
  obj := TJsonObject.Create;
  obj.S['d'] := aText;
  try
    Log(aTag, 'value=' + FloatToStr(obj.D['d']) + ' types=' + IntToStr(obj.Types['d']));
  except on E: Exception do
    Log(aTag, 'exc=' + E.ClassName + ' ' + E.Message);
  end;
  obj.Free;
end;

procedure Numerics;
var
  y, mo, d, h, mi, sec, ms: cardinal;
begin
  Log('enc', '1899-12-29=' + FloatToStr(EncodeDate(1899, 12, 29)) + ' 1899-12-30=' + FloatToStr(EncodeDate(1899, 12, 30))
    + ' 1899-12-31=' + FloatToStr(EncodeDate(1899, 12, 31)));
  DecodeDate(-1.25, y, mo, d);
  Log('dec', 'dd(-1.25)=' + IntToStr(y) + '-' + IntToStr(mo) + '-' + IntToStr(d));
  DecodeDate(-0.25, y, mo, d);
  Log('dec', 'dd(-0.25)=' + IntToStr(y) + '-' + IntToStr(mo) + '-' + IntToStr(d));
  DecodeDate(-0.75, y, mo, d);
  Log('dec', 'dd(-0.75)=' + IntToStr(y) + '-' + IntToStr(mo) + '-' + IntToStr(d));
  DecodeTime(-1.25, h, mi, sec, ms);
  Log('dec', 'dt(-1.25)=' + IntToStr(h) + ':' + IntToStr(mi) + ':' + IntToStr(sec) + '.' + IntToStr(ms));
  DecodeTime(-0.7916666666, h, mi, sec, ms);
  Log('dec', 'dt(-0.79166)=' + IntToStr(h) + ':' + IntToStr(mi) + ':' + IntToStr(sec) + '.' + IntToStr(ms));
  DecodeTime(-1.7916666666, h, mi, sec, ms);
  Log('dec', 'dt(-1.79166)=' + IntToStr(h) + ':' + IntToStr(mi) + ':' + IntToStr(sec) + '.' + IntToStr(ms));
  Log('off', '-1.25-u=' + FloatToStr(-1.25 - (EncodeDate(1899, 12, 29) + EncodeTime(13, 0, 0, 0)))
    + ' -0.25-u=' + FloatToStr(-0.25 - (EncodeDate(1899, 12, 30) + EncodeTime(13, 0, 0, 0)))
    + ' -0.75-u=' + FloatToStr(-0.75 - (EncodeDate(1899, 12, 31) + EncodeTime(1, 0, 0, 0))));
end;

function Initialize: integer;
var
  obj: TJsonObject;
begin
  Result := 0;
  Numerics;
  // The value kinds DateTimeValue reads.
  obj := TJsonObject.Create;
  Log('none', FloatToStr(obj.D['missing']));
  obj.I['i'] := 3;
  obj.L['l'] := 4;
  obj.U['u'] := 5;
  obj.F['f'] := 1.5;
  obj.B['b'] := True;
  obj.S['s'] := '2015-02-01T16:08:19.202Z';
  Log('kinds', 'i=' + FloatToStr(obj.D['i']) + ' l=' + FloatToStr(obj.D['l']) + ' u=' + FloatToStr(obj.D['u'])
    + ' f=' + FloatToStr(obj.D['f']) + ' b=' + FloatToStr(obj.D['b']));
  Log('types', 'i=' + IntToStr(obj.Types['i']) + ' s=' + IntToStr(obj.Types['s']) + ' b=' + IntToStr(obj.Types['b']));
  obj.Free;
  // A null object reads as a cast error (NullConvertsToValueTypes is off).
  obj := TJsonBaseObject.Parse('{ "o": null }');
  try
    Log('null', 'value=' + FloatToStr(obj.D['o']));
  except on E: Exception do
    Log('null', 'exc=' + E.ClassName + ' ' + E.Message);
  end;
  Log('null', 'types=' + IntToStr(obj.Types['o']));
  obj.Free;
  // The text of a stored value, with the local bias on and off.
  WriteValue('2015-02-01', EncodeDate(2015, 2, 1) + EncodeTime(16, 8, 19, 202));
  WriteValue('winter', EncodeDate(2015, 1, 15) + EncodeTime(12, 0, 0, 0));
  WriteValue('summer', EncodeDate(2015, 7, 15) + EncodeTime(12, 0, 0, 0));
  WriteValue('spring-gap', EncodeDate(2015, 3, 8) + EncodeTime(2, 30, 0, 0));
  WriteValue('autumn-ambiguous', EncodeDate(2015, 11, 1) + EncodeTime(1, 30, 0, 0));
  WriteValue('epoch', EncodeDate(1970, 1, 1));
  WriteValue('negative', -0.25);
  WriteValue('negative-day', -1.25);
  WriteValue('negative-half', -0.75);
  // The parsing surface.
  ParseValue('parse-z', '2015-02-01T16:08:19.202Z');
  ParseValue('parse-offset', '2015-02-01T16:08:19.202+02:00');
  ParseValue('parse-neg', '2015-02-01T16:08:19.202-05:00');
  ParseValue('parse-plain', '2015-02-01T16:08:19.202');
  ParseValue('parse-dateonly', '2015-02-01');
  ParseValue('parse-dotnet', '/Date(1422845299202)/');
  ParseValue('parse-empty', '');
  ParseValue('parse-garbage', 'not a date');
  ParseValue('parse-short', '2015');
  ParseValue('parse-zero', '0000-01-01');
  ParseValue('parse-badmonth', '2015-13-01');
  // The local conversion off: the string read is the UTC value.
  ParseValue('parse-utc', '2015-07-15T12:00:00Z');
  // The local conversion of the data code itself: each string's UTC value is
  // converted to the machine's local time and back through SystemTimeToDateTime,
  // which some of these leave before the TDateTime epoch.
  ParseValue('parse-1899a', '1899-12-30T01:00:00Z');
  ParseValue('parse-1899b', '1899-12-29T13:00:00Z');
  ParseValue('parse-1899c', '1899-12-30T08:00:00Z');
  ParseValue('parse-1899d', '1899-12-31T02:00:00Z');
  ParseValue('parse-1899e', '1899-12-30T00:00:00Z');
end;

end.
