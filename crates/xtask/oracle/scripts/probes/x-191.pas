{ parity probe: x-191 }
unit P_x_191;

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
  v := not 1.5;
  AddMessage('after');
end;
end.
