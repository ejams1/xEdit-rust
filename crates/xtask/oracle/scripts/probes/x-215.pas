{ parity probe: x-215 }
unit P_x_215;

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
  v := sl + 'x';
  AddMessage('after');
end;
end.
