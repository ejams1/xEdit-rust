{ parity probe: f-24 }
unit P_f_24;

function Initialize: integer;
var
  lst: TList;
  sl: TStringList;
  s: string;
  i64: Int64;
  c: Cardinal;
  d: Double;
begin
  lst := TList.Create;
  sl := TStringList.Create;
  sl.Add('a');
  i64 := 5;
  c := 5;
  d := 1.5;
  s := Format('%s %s this is a long format string over 31 chars', ['a']);
  AddMessage('[' + s + ']');
end;
end.
