{ parity probe: x-152 }
unit P_x_152;

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
  v := sl.Count; v := v + 'x';
  AddMessage('after');
end;
end.
