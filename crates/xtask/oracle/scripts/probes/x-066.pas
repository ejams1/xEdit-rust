{ parity probe: x-066 }
unit P_x_066;

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
  v := Name(5);
  AddMessage('after');
end;
end.
