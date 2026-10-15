{ parity probe: x-355 }
unit P_x_355;

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
  v := TList.Create; v.Add(5); v[1] := 6;
  AddMessage('after');
end;
end.
