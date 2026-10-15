{ parity probe: x-311 }
unit P_x_311;

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
  sl.Delete('x');
  AddMessage('after');
end;
end.
