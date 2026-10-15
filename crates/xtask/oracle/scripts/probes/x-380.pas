{ parity probe: x-380 }
unit P_x_380;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := TList.Create; v.Add(5); v.Sort(nil);
  AddMessage('after');
end;
end.
