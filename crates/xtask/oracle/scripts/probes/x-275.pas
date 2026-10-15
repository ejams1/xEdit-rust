{ parity probe: x-275 }
unit P_x_275;

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
  v := UpperCase(Null); AddMessage('[' + v + ']');
  AddMessage('after');
end;
end.
