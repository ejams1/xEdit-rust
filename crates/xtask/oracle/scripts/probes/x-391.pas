{ parity probe: x-391 }
unit P_x_391;

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
  v := TList.Create; v.Add(5); v.Add(6); v.Delete(-1);
  AddMessage('after');
end;
end.
