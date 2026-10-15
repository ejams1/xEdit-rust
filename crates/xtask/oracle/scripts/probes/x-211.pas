{ parity probe: x-211 }
unit P_x_211;

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
  v := 5 = sl;
  AddMessage('after');
end;
end.
