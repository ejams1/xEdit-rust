{ parity probe: x-174 }
unit P_x_174;

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
  v := Null; if v then AddMessage('yes') else AddMessage('no');
  AddMessage('after');
end;
end.
