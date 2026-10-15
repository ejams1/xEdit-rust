{ parity probe: f-79 }
unit P_f_79;

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
  s := Format('%3d', [5]);
  AddMessage('[' + s + ']');
end;
end.
