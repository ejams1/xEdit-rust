{ parity probe: x-329 }
unit P_x_329;

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
  v := sl as TList;
  AddMessage('after');
end;
end.
