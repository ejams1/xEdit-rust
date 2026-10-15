{ parity probe: x-031 }
unit P_x_031;

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
  if 5 then ;
  AddMessage('after');
end;
end.
