{ parity probe: x-199 }
unit P_x_199;

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
  v := -sl;
  AddMessage('after');
end;
end.
