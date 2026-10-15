{ parity probe: f-82 }
unit P_f_82;

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
  s := Format('%d', [High(sl.Count)]);
  AddMessage('[' + s + ']');
end;
end.
